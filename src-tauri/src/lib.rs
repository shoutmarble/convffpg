use convffpg::editor_export::EditorExportOutcome;
use convffpg::ffmpeg::BundledFfmpeg;
use convffpg::session::{LoadedSession, ProjectDocument, RecentSessionSummary};

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserRoot {
    key: String,
    label: String,
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserEntry {
    name: String,
    path: String,
    is_directory: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserListing {
    path: String,
    parent_path: Option<String>,
    entries: Vec<BrowserEntry>,
}

#[derive(Debug, Serialize)]
struct SessionPayload {
    manifest: convffpg::session::SessionManifest,
    project: ProjectDocument,
    source_video_path: String,
}

impl SessionPayload {
    fn from_loaded(loaded: LoadedSession) -> Self {
        let source_video_path = convffpg::source_video_path(&loaded.manifest)
            .display()
            .to_string();

        Self {
            manifest: loaded.manifest,
            project: loaded.project,
            source_video_path,
        }
    }
}

#[tauri::command]
fn ffmpeg_status() -> Result<BundledFfmpeg, String> {
    convffpg::ensure_bundled_ffmpeg()
}

#[tauri::command]
fn import_mp4(source_path: String) -> Result<SessionPayload, String> {
    let loaded = convffpg::import_media_session(Path::new(&source_path))?;
    Ok(SessionPayload::from_loaded(loaded))
}

#[tauri::command]
fn open_session(manifest_path: String) -> Result<SessionPayload, String> {
    let loaded = convffpg::open_existing_session(Path::new(&manifest_path))?;
    Ok(SessionPayload::from_loaded(loaded))
}

#[tauri::command]
fn list_recent_sessions(limit: Option<usize>) -> Result<Vec<RecentSessionSummary>, String> {
    convffpg::recent_sessions(limit.unwrap_or(8))
}

#[tauri::command]
fn save_project_document(project: ProjectDocument) -> Result<(), String> {
    convffpg::session::save_project(&project)
}

#[tauri::command]
fn export_project(manifest_path: String) -> Result<EditorExportOutcome, String> {
    convffpg::export_session(Path::new(&manifest_path))
}

#[tauri::command]
fn list_browser_roots() -> Result<Vec<BrowserRoot>, String> {
    allowed_browser_roots().map(|roots| {
        roots.into_iter()
            .map(|(key, label, path)| BrowserRoot {
                key,
                label,
                path: path.display().to_string(),
            })
            .collect()
    })
}

#[tauri::command]
fn list_browser_directory(path: String) -> Result<BrowserListing, String> {
    let requested_path = resolve_browser_path(&path)?;
    let roots = allowed_browser_roots()?;

    let mut entries = Vec::new();
    for entry in fs::read_dir(&requested_path)
        .map_err(|error| format!("failed to read {}: {error}", requested_path.display()))?
    {
        let entry = entry.map_err(|error| {
            format!("failed to inspect an entry in {}: {error}", requested_path.display())
        })?;
        let entry_path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", entry_path.display()))?;

        let resolved_entry_path = match fs::canonicalize(&entry_path) {
            Ok(path) if is_allowed_browser_path(&path, &roots) => path,
            Ok(_) => continue,
            Err(_) => continue,
        };

        entries.push(BrowserEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path: resolved_entry_path.display().to_string(),
            is_directory: metadata.is_dir(),
        });
    }

    entries.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let parent_path = requested_path.parent().and_then(|parent| {
        let parent_buf = parent.to_path_buf();
        is_allowed_browser_path(&parent_buf, &roots).then(|| parent_buf.display().to_string())
    });

    Ok(BrowserListing {
        path: requested_path.display().to_string(),
        parent_path,
        entries,
    })
}

fn allowed_browser_roots() -> Result<Vec<(String, String, PathBuf)>, String> {
    let mut roots = Vec::new();

    if let Some(downloads) = dirs::download_dir() {
        if downloads.exists() {
            roots.push((
                String::from("downloads"),
                String::from("Downloads"),
                canonicalize_browser_root(downloads)?,
            ));
        }
    }

    if let Some(videos) = dirs::video_dir() {
        if videos.exists() {
            roots.push((
                String::from("videos"),
                String::from("Videos"),
                canonicalize_browser_root(videos)?,
            ));
        }
    }

    let sessions = session_browser_root()?;
    roots.push((
        String::from("sessions"),
        String::from("Sessions"),
        canonicalize_browser_root(sessions)?,
    ));

    roots.sort_by(|left, right| left.1.cmp(&right.1));
    roots.dedup_by(|left, right| left.2 == right.2);

    Ok(roots)
}

fn session_browser_root() -> Result<PathBuf, String> {
    let base_dir = dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|dir| dir.join(".local").join("share")))
        .ok_or_else(|| String::from("could not determine a local data directory for browser roots"))?;

    let sessions = base_dir.join("convffpg").join("recording-sessions");
    fs::create_dir_all(&sessions)
        .map_err(|error| format!("failed to create {}: {error}", sessions.display()))?;
    Ok(sessions)
}

fn canonicalize_browser_root(path: PathBuf) -> Result<PathBuf, String> {
    fs::canonicalize(&path).map_err(|error| format!("failed to resolve {}: {error}", path.display()))
}

fn resolve_browser_path(path: &str) -> Result<PathBuf, String> {
    let requested = PathBuf::from(path);
    let canonical = fs::canonicalize(&requested)
        .map_err(|error| format!("failed to resolve {}: {error}", requested.display()))?;
    let roots = allowed_browser_roots()?;

    if is_allowed_browser_path(&canonical, &roots) {
        Ok(canonical)
    } else {
        Err(format!("{} is outside the browser roots", requested.display()))
    }
}

fn is_allowed_browser_path(path: &Path, roots: &[(String, String, PathBuf)]) -> bool {
    roots.iter().any(|(_, _, root)| path.starts_with(root))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ffmpeg_status,
            import_mp4,
            open_session,
            list_recent_sessions,
            list_browser_roots,
            list_browser_directory,
            save_project_document,
            export_project
        ])
        .run(tauri::generate_context!())
        .expect("failed to run convffpg tauri shell");
}