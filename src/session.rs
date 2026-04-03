use serde::{Deserialize, Serialize};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const APP_DIR_NAME: &str = "convffpg";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifest {
    pub version: u32,
    pub session_id: String,
    pub created_at_unix_ms: u64,
    pub stage: SessionStage,
    pub source: SessionSource,
    pub microphone_enabled: bool,
    pub files: SessionFiles,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSource {
    pub id: String,
    pub name: String,
    pub kind: CaptureSourceKind,
    pub detail: String,
    pub capture_area: Option<CaptureArea>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaptureSourceKind {
    Display,
    Window,
    Region,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStage {
    Draft,
    Prepared,
    Recording,
    Editing,
    Exporting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFiles {
    pub session_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub project_path: PathBuf,
    pub screen_capture_path: PathBuf,
    pub webcam_capture_path: Option<PathBuf>,
    pub exported_gif_path: PathBuf,
    pub exported_mp4_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDocument {
    pub version: u32,
    pub session_id: String,
    pub created_at_unix_ms: u64,
    pub timeline_tracks: Vec<TimelineTrack>,
    pub export: ExportSettings,
    pub composite: CompositeSettings,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineTrack {
    pub id: String,
    pub label: String,
    pub kind: TimelineTrackKind,
    pub regions: Vec<TimelineRegion>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimelineTrackKind {
    Trim,
    Speed,
    Zoom,
    Annotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineRegion {
    pub id: String,
    pub label: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub emphasis: Option<f32>,
    #[serde(default)]
    pub focus_rect: Option<NormalizedRect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSettings {
    pub primary_format: ExportFormat,
    pub gif_fps: u32,
    pub loop_gif: bool,
    pub target_height: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExportFormat {
    Gif,
    Mp4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeSettings {
    pub aspect_ratio: String,
    pub background_style: String,
    pub webcam_layout: String,
    pub cursor_highlight: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedSession {
    pub manifest: SessionManifest,
    pub project: ProjectDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentSessionSummary {
    pub session_id: String,
    pub manifest_path: PathBuf,
    pub source_name: String,
    pub stage: SessionStage,
    pub updated_at_unix_ms: u64,
}

pub fn create_draft(source: SessionSource, microphone_enabled: bool) -> Result<SessionManifest, String> {
    let created_at_unix_ms = unix_timestamp_ms()?;
    let session_id = format!("session-{created_at_unix_ms}");
    let sessions_root = sessions_root()?;
    let session_dir = sessions_root.join(&session_id);

    fs::create_dir_all(&session_dir).map_err(|error| {
        format!(
            "failed to create the recording session directory at {}: {error}",
            session_dir.display()
        )
    })?;

    let manifest_path = session_dir.join("session.json");
    let project_path = session_dir.join("project.json");

    let manifest = SessionManifest {
        version: 1,
        session_id,
        created_at_unix_ms,
        stage: SessionStage::Prepared,
        source,
        microphone_enabled,
        files: SessionFiles {
            session_dir: session_dir.clone(),
            manifest_path: manifest_path.clone(),
            project_path: project_path.clone(),
            screen_capture_path: session_dir.join("screen-capture.mp4"),
            webcam_capture_path: None,
            exported_gif_path: session_dir.join("export.gif"),
            exported_mp4_path: session_dir.join("export.mp4"),
        },
        notes: vec![
            String::from("Draft session created before the capture pipeline is implemented."),
            String::from("This manifest is the persistence model for the future recorder/editor flow."),
        ],
    };

    save_manifest(&manifest)?;
    save_project(&default_project(&manifest))?;

    Ok(manifest)
}

pub fn save_manifest(manifest: &SessionManifest) -> Result<(), String> {
    let payload = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("failed to serialize the session manifest: {error}"))?;

    fs::write(&manifest.files.manifest_path, payload).map_err(|error| {
        format!(
            "failed to write the session manifest to {}: {error}",
            manifest.files.manifest_path.display()
        )
    })
}

pub fn save_project(project: &ProjectDocument) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(project)
        .map_err(|error| format!("failed to serialize the project document: {error}"))?;

    let project_path = sessions_root()?.join(&project.session_id).join("project.json");

    fs::write(&project_path, pretty).map_err(|error| {
        format!(
            "failed to write the project document to {}: {error}",
            project_path.display()
        )
    })
}

pub fn load_manifest(path: &Path) -> Result<SessionManifest, String> {
    let payload = fs::read_to_string(path)
        .map_err(|error| format!("failed to read the session manifest at {}: {error}", path.display()))?;

    serde_json::from_str(&payload)
        .map_err(|error| format!("failed to parse the session manifest at {}: {error}", path.display()))
}

pub fn load_project(path: &Path) -> Result<ProjectDocument, String> {
    let payload = fs::read_to_string(path)
        .map_err(|error| format!("failed to read the project document at {}: {error}", path.display()))?;

    serde_json::from_str(&payload)
        .map_err(|error| format!("failed to parse the project document at {}: {error}", path.display()))
}

pub fn open_session(manifest_path: &Path) -> Result<LoadedSession, String> {
    let manifest = load_manifest(manifest_path)?;

    let mut project = if manifest.files.project_path.exists() {
        load_project(&manifest.files.project_path)?
    } else {
        let project = default_project(&manifest);
        save_project(&project)?;
        project
    };

    if normalize_project_tracks(&mut project) {
        save_project(&project)?;
    }

    Ok(LoadedSession { manifest, project })
}

pub fn list_recent_sessions(limit: usize) -> Result<Vec<RecentSessionSummary>, String> {
    let mut sessions = Vec::new();

    for entry in fs::read_dir(sessions_root()?)
        .map_err(|error| format!("failed to read the recording sessions directory: {error}"))?
    {
        let entry = entry.map_err(|error| format!("failed to inspect a recording session directory: {error}"))?;
        let session_dir = entry.path();

        if !session_dir.is_dir() {
            continue;
        }

        let manifest_path = session_dir.join("session.json");

        if !manifest_path.exists() {
            continue;
        }

        let manifest = match load_manifest(&manifest_path) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };

        let updated_at_unix_ms = modified_time_ms(&manifest_path).unwrap_or(manifest.created_at_unix_ms);

        sessions.push(RecentSessionSummary {
            session_id: manifest.session_id,
            manifest_path,
            source_name: manifest.source.name,
            stage: manifest.stage,
            updated_at_unix_ms,
        });
    }

    sessions.sort_by(|left, right| right.updated_at_unix_ms.cmp(&left.updated_at_unix_ms));
    sessions.truncate(limit);

    Ok(sessions)
}

pub fn update_stage(manifest: &mut SessionManifest, stage: SessionStage, note: impl Into<String>) -> Result<(), String> {
    manifest.stage = stage;
    manifest.notes.push(note.into());
    save_manifest(manifest)
}

pub fn append_project_note(project: &mut ProjectDocument, note: impl Into<String>) -> Result<(), String> {
    project.notes.push(note.into());
    save_project(project)
}

fn default_project(manifest: &SessionManifest) -> ProjectDocument {
    ProjectDocument {
        version: manifest.version,
        session_id: manifest.session_id.clone(),
        created_at_unix_ms: manifest.created_at_unix_ms,
        timeline_tracks: vec![
            TimelineTrack {
                id: String::from("trim-track"),
                label: String::from("Trim"),
                kind: TimelineTrackKind::Trim,
                regions: vec![],
            },
            TimelineTrack {
                id: String::from("speed-track"),
                label: String::from("Speed"),
                kind: TimelineTrackKind::Speed,
                regions: vec![],
            },
            TimelineTrack {
                id: String::from("zoom-track"),
                label: String::from("Magnify"),
                kind: TimelineTrackKind::Zoom,
                regions: vec![],
            },
            TimelineTrack {
                id: String::from("annotation-track"),
                label: String::from("Text"),
                kind: TimelineTrackKind::Annotation,
                regions: vec![],
            },
        ],
        export: ExportSettings {
            primary_format: ExportFormat::Gif,
            gif_fps: 12,
            loop_gif: true,
            target_height: 720,
        },
        composite: CompositeSettings {
            aspect_ratio: String::from("16:9"),
            background_style: String::from("Solid"),
            webcam_layout: String::from("Off"),
            cursor_highlight: true,
        },
        notes: vec![
            String::from("Project scaffold created for the Rust recorder/editor workflow."),
            String::from("Timeline tracks are persisted even before media capture is implemented on every platform."),
        ],
    }
}

fn normalize_project_tracks(project: &mut ProjectDocument) -> bool {
    let mut changed = false;

    for track in &mut project.timeline_tracks {
        let desired_label = match track.kind {
            TimelineTrackKind::Trim => "Trim",
            TimelineTrackKind::Speed => "Speed",
            TimelineTrackKind::Zoom => "Magnify",
            TimelineTrackKind::Annotation => "Text",
        };

        if track.label != desired_label {
            track.label = String::from(desired_label);
            changed = true;
        }
    }

    changed
}

fn sessions_root() -> Result<PathBuf, String> {
    let base_dir = dirs::data_local_dir().or_else(fallback_local_share_dir);

    let root = base_dir
        .map(|dir| dir.join(APP_DIR_NAME).join("recording-sessions"))
        .ok_or_else(|| String::from("could not determine a local data directory for recording sessions"))?;

    fs::create_dir_all(&root).map_err(|error| {
        format!(
            "failed to create the recording sessions root at {}: {error}",
            root.display()
        )
    })?;

    Ok(root)
}

fn fallback_local_share_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|dir| dir.join(".local").join("share"))
}

fn modified_time_ms(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;

    Some(duration.as_millis() as u64)
}

fn unix_timestamp_ms() -> Result<u64, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?;

    Ok(now.as_millis() as u64)
}