use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tar::Archive;
use tempfile::Builder;
use walkdir::WalkDir;
use xz2::read::XzDecoder;

const APP_DIR_NAME: &str = "convffpg";
const BUNDLE_VERSION: &str = "linux-x86_64-abda8d77ce83";
const FFMPEG_ARCHIVE: &[u8] = include_bytes!("../assets/ffmpeg/ffmpeg-linux-x86_64.tar.xz");

#[derive(Debug, Clone, Serialize)]
pub struct BundledFfmpeg {
    pub install_dir: PathBuf,
    pub binary_path: PathBuf,
}

pub fn ensure_ready() -> Result<BundledFfmpeg, String> {
    let bundles_root = bundles_root()?;
    let install_dir = bundles_root.join(BUNDLE_VERSION);

    if let Some(binary_path) = existing_binary(&install_dir) {
        return Ok(BundledFfmpeg {
            install_dir,
            binary_path,
        });
    }

    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).map_err(|error| {
            format!(
                "failed to remove a stale FFmpeg install at {}: {error}",
                install_dir.display()
            )
        })?;
    }

    fs::create_dir_all(&bundles_root).map_err(|error| {
        format!(
            "failed to create the FFmpeg bundle directory at {}: {error}",
            bundles_root.display()
        )
    })?;

    let staging_dir = Builder::new()
        .prefix("ffmpeg-unpack-")
        .tempdir_in(&bundles_root)
        .map_err(|error| format!("failed to create a temporary extraction directory: {error}"))?;

    let decoder = XzDecoder::new(Cursor::new(FFMPEG_ARCHIVE));
    let mut archive = Archive::new(decoder);

    archive.unpack(staging_dir.path()).map_err(|error| {
        format!(
            "failed to unpack the embedded FFmpeg archive into {}: {error}",
            staging_dir.path().display()
        )
    })?;

    let unpacked_dir = top_level_directory(staging_dir.path())?;
    let staged_binary = find_ffmpeg_binary(&unpacked_dir)?;
    ensure_executable(&staged_binary)?;

    fs::rename(&unpacked_dir, &install_dir).map_err(|error| {
        format!(
            "failed to move the extracted FFmpeg bundle into {}: {error}",
            install_dir.display()
        )
    })?;

    let binary_path = find_ffmpeg_binary(&install_dir)?;

    Ok(BundledFfmpeg {
        install_dir,
        binary_path,
    })
}

fn bundles_root() -> Result<PathBuf, String> {
    let base_dir = dirs::data_local_dir().or_else(fallback_local_share_dir);

    base_dir
        .map(|dir| dir.join(APP_DIR_NAME).join("bundled-ffmpeg"))
        .ok_or_else(|| String::from("could not determine a local data directory for FFmpeg"))
}

fn fallback_local_share_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|dir| dir.join(".local").join("share"))
}

fn existing_binary(install_dir: &Path) -> Option<PathBuf> {
    if !install_dir.exists() {
        return None;
    }

    find_ffmpeg_binary(install_dir).ok()
}

fn top_level_directory(staging_root: &Path) -> Result<PathBuf, String> {
    let mut candidates = fs::read_dir(staging_root)
        .map_err(|error| {
            format!(
                "failed to inspect extracted FFmpeg files in {}: {error}",
                staging_root.display()
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());

    candidates.next().ok_or_else(|| {
        format!(
            "the embedded FFmpeg archive did not contain an extracted top-level directory under {}",
            staging_root.display()
        )
    })
}

fn find_ffmpeg_binary(root: &Path) -> Result<PathBuf, String> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;

            if entry.file_type().is_file() && name == "ffmpeg" {
                Some(path.to_path_buf())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            format!(
                "the extracted FFmpeg bundle in {} does not contain an ffmpeg binary",
                root.display()
            )
        })
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?
        .permissions();

    permissions.set_mode(0o755);

    fs::set_permissions(path, permissions)
        .map_err(|error| format!("failed to mark {} as executable: {error}", path.display()))
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}