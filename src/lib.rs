pub mod conversion;
pub mod editor_export;
pub mod ffmpeg;
pub mod session;

use crate::editor_export::EditorExportOutcome;
use crate::ffmpeg::BundledFfmpeg;
use crate::session::{
    CaptureSourceKind, LoadedSession, RecentSessionSummary, SessionManifest, SessionSource,
    SessionStage,
};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn ensure_bundled_ffmpeg() -> Result<BundledFfmpeg, String> {
    ffmpeg::ensure_ready()
}

pub fn import_media_session(source_path: &Path) -> Result<LoadedSession, String> {
    let source_path = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        source_path
            .canonicalize()
            .map_err(|error| format!("failed to resolve {}: {error}", source_path.display()))?
    };

    if !source_path.exists() {
        return Err(format!(
            "the selected media file does not exist: {}",
            source_path.display()
        ));
    }

    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    if extension != "mp4" {
        return Err(String::from("only MP4 imports are supported in the Tauri workflow right now."));
    }

    let source = SessionSource {
        id: format!("imported-{}", unix_timestamp_ms()?),
        name: source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(String::from)
            .unwrap_or_else(|| String::from("Imported MP4")),
        kind: CaptureSourceKind::Display,
        detail: source_path.display().to_string(),
        capture_area: None,
    };

    let mut manifest = session::create_draft(source, false)?;

    fs::copy(&source_path, &manifest.files.screen_capture_path).map_err(|error| {
        format!(
            "failed to copy the imported MP4 into {}: {error}",
            manifest.files.screen_capture_path.display()
        )
    })?;

    manifest.stage = SessionStage::Editing;
    manifest.notes.push(format!(
        "Imported source media from {}.",
        source_path.display()
    ));
    session::save_manifest(&manifest)?;

    session::open_session(&manifest.files.manifest_path)
}

pub fn open_existing_session(manifest_path: &Path) -> Result<LoadedSession, String> {
    session::open_session(manifest_path)
}

pub fn recent_sessions(limit: usize) -> Result<Vec<RecentSessionSummary>, String> {
    session::list_recent_sessions(limit)
}

pub fn export_session(manifest_path: &Path) -> Result<EditorExportOutcome, String> {
    let bundled = ffmpeg::ensure_ready()?;
    let loaded = session::open_session(manifest_path)?;

    editor_export::export_project(bundled.binary_path, loaded.manifest, loaded.project)
}

pub fn source_video_path(manifest: &SessionManifest) -> PathBuf {
    manifest.files.screen_capture_path.clone()
}

fn unix_timestamp_ms() -> Result<u64, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?;

    Ok(now.as_millis() as u64)
}