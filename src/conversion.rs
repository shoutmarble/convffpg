use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::tempdir;

const GIF_FPS: &str = "12";
const GIF_SCALE: &str = "720:-1:flags=lanczos";

#[derive(Debug, Clone)]
pub struct ConversionJob {
    ffmpeg_path: PathBuf,
    input_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DetectionOutcome {
    pub job: ConversionJob,
    pub output_path: PathBuf,
    pub detected_frames: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ConversionOutcome {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub detected_frames: Option<u64>,
}

impl ConversionJob {
    pub fn new(ffmpeg_path: PathBuf, input_path: PathBuf) -> Self {
        Self {
            ffmpeg_path,
            input_path,
        }
    }
}

pub fn detect_frames(job: ConversionJob) -> Result<DetectionOutcome, String> {
    if !job.input_path.exists() {
        return Err(format!(
            "the input file does not exist: {}",
            job.input_path.display()
        ));
    }

    let output_path = unique_output_path(&job.input_path);
    let detected_frames = detect_frame_count(&job)?;

    Ok(DetectionOutcome {
        job,
        output_path,
        detected_frames,
    })
}

pub fn render_gif(detection: DetectionOutcome) -> Result<ConversionOutcome, String> {
    let temp_dir = tempdir().map_err(|error| format!("failed to create a temp directory: {error}"))?;
    let palette_path = temp_dir.path().join("palette.png");

    run_ffmpeg(
        &detection.job.ffmpeg_path,
        [
            "-y".into(),
            "-i".into(),
            detection.job.input_path.display().to_string(),
            "-vf".into(),
            format!("fps={GIF_FPS},scale={GIF_SCALE},palettegen=stats_mode=diff"),
            palette_path.display().to_string(),
        ],
    )?;

    run_ffmpeg(
        &detection.job.ffmpeg_path,
        [
            "-y".into(),
            "-i".into(),
            detection.job.input_path.display().to_string(),
            "-i".into(),
            palette_path.display().to_string(),
            "-lavfi".into(),
            format!(
                "fps={GIF_FPS},scale={GIF_SCALE}[clip];[clip][1:v]paletteuse=dither=sierra2_4a"
            ),
            "-loop".into(),
            "0".into(),
            detection.output_path.display().to_string(),
        ],
    )?;

    Ok(ConversionOutcome {
        input_path: detection.job.input_path,
        output_path: detection.output_path,
        detected_frames: detection.detected_frames,
    })
}

pub fn is_supported_input(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
}

fn unique_output_path(input_path: &Path) -> PathBuf {
    let parent = input_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("converted");

    for index in 0.. {
        let suffix = if index == 0 {
            String::from("-animated.gif")
        } else {
            format!("-animated-{index}.gif")
        };

        let candidate = parent.join(format!("{stem}{suffix}"));

        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("the output path generator should always return a path")
}

fn detect_frame_count(job: &ConversionJob) -> Result<Option<u64>, String> {
    let ffprobe_path = ffprobe_path(&job.ffmpeg_path);

    if !ffprobe_path.exists() {
        return Ok(None);
    }

    let output = Command::new(&ffprobe_path)
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=nokey=1:noprint_wrappers=1",
        ])
        .arg(&job.input_path)
        .output()
        .map_err(|error| format!("failed to launch {}: {error}", ffprobe_path.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout.trim();

    if value.is_empty() || value.eq_ignore_ascii_case("N/A") {
        return Ok(None);
    }

    value
        .parse::<u64>()
        .map(Some)
        .map_err(|error| format!("failed to parse frame count from {}: {error}", ffprobe_path.display()))
}

fn ffprobe_path(ffmpeg_path: &Path) -> PathBuf {
    ffmpeg_path.with_file_name("ffprobe")
}

fn run_ffmpeg(
    ffmpeg_path: &Path,
    args: impl IntoIterator<Item = String>,
) -> Result<(), String> {
    let output = Command::new(ffmpeg_path)
        .args(args)
        .output()
        .map_err(|error| format!("failed to launch {}: {error}", ffmpeg_path.display()))?;

    ensure_success(output, ffmpeg_path)
}

fn ensure_success(output: Output, ffmpeg_path: &Path) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    Err(format!(
        "{} exited with {}: {}",
        ffmpeg_path.display(),
        output.status,
        stderr.trim()
    ))
}