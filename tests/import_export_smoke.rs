use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn imported_mp4_round_trip_exports_mp4() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempdir()?;
    let input_path = temp_dir.path().join("sample.mp4");

    create_sample_mp4(&input_path)?;

    let loaded = convffpg::import_media_session(&input_path).map_err(io::Error::other)?;

    let reopened = convffpg::open_existing_session(&loaded.manifest.files.manifest_path)
        .map_err(io::Error::other)?;
    assert_eq!(reopened.manifest.session_id, loaded.manifest.session_id);
    assert!(reopened.manifest.files.project_path.exists());

    let export = convffpg::export_session(&loaded.manifest.files.manifest_path)
        .map_err(io::Error::other)?;
    assert!(export.output_path.exists());
    assert_eq!(export.output_path, reopened.manifest.files.exported_gif_path);

    fs::remove_dir_all(&loaded.manifest.files.session_dir)?;

    Ok(())
}

fn create_sample_mp4(output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x1f2937:s=640x360:d=1.2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(output_path)
        .status()?;

    if !status.success() {
        return Err(Box::new(io::Error::other(format!(
            "ffmpeg failed to create sample video: {status}"
        ))));
    }

    Ok(())
}