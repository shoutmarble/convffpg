use convffpg::editor_export::EditorExportOutcome;
use convffpg::ffmpeg::BundledFfmpeg;
use convffpg::session::{LoadedSession, ProjectDocument, RecentSessionSummary};

use serde::Serialize;
use std::path::Path;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ffmpeg_status,
            import_mp4,
            open_session,
            list_recent_sessions,
            save_project_document,
            export_project
        ])
        .run(tauri::generate_context!())
        .expect("failed to run convffpg tauri shell");
}