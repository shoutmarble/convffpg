use crate::session::{
    ExportFormat, NormalizedRect, ProjectDocument, SessionManifest, TimelineRegion,
    TimelineTrackKind,
};

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::tempdir;

#[derive(Debug, Clone, Serialize)]
pub struct EditorExportOutcome {
    pub output_path: PathBuf,
    pub intermediate_mp4_path: PathBuf,
}

#[derive(Debug, Clone)]
struct ChunkPlan {
    start_ms: u64,
    end_ms: u64,
    speed: f32,
    magnify: Option<f32>,
    focus_rect: Option<NormalizedRect>,
    text: Option<String>,
}

pub fn export_project(
    ffmpeg_path: PathBuf,
    manifest: SessionManifest,
    project: ProjectDocument,
) -> Result<EditorExportOutcome, String> {
    let input_path = manifest.files.screen_capture_path.clone();

    if !input_path.exists() {
        return Err(format!(
            "the session media does not exist yet: {}",
            input_path.display()
        ));
    }

    let duration_ms = probe_duration_ms(&ffmpeg_path, &input_path)?;
    let chunks = build_chunk_plan(&project, duration_ms);

    if chunks.is_empty() {
        return Err(String::from(
            "every segment is currently trimmed away, so there is nothing left to export.",
        ));
    }

    let temp_dir = tempdir().map_err(|error| format!("failed to create an export temp directory: {error}"))?;
    let temp_root = temp_dir.path();
    let target_width = aspect_ratio_width(project.export.target_height, &project.composite.aspect_ratio);
    let target_height = project.export.target_height;

    let mut chunk_paths = Vec::new();

    for (index, chunk) in chunks.iter().enumerate() {
        let chunk_path = temp_root.join(format!("chunk-{index:03}.mp4"));
        render_chunk(
            &ffmpeg_path,
            &input_path,
            &chunk_path,
            chunk,
            target_width,
            target_height,
        )?;
        chunk_paths.push(chunk_path);
    }

    let intermediate_mp4_path = temp_root.join("edited-output.mp4");
    concat_chunks(&ffmpeg_path, &chunk_paths, &intermediate_mp4_path, temp_root)?;

    let final_mp4_path = manifest.files.exported_mp4_path.clone();
    fs::copy(&intermediate_mp4_path, &final_mp4_path).map_err(|error| {
        format!(
            "failed to write the MP4 export to {}: {error}",
            final_mp4_path.display()
        )
    })?;

    match project.export.primary_format {
        ExportFormat::Mp4 => Ok(EditorExportOutcome {
            output_path: final_mp4_path,
            intermediate_mp4_path,
        }),
        ExportFormat::Gif => {
            let gif_path = manifest.files.exported_gif_path.clone();
            render_gif_from_mp4(
                &ffmpeg_path,
                &intermediate_mp4_path,
                &gif_path,
                project.export.gif_fps,
                project.export.loop_gif,
            )?;

            Ok(EditorExportOutcome {
                output_path: gif_path,
                intermediate_mp4_path,
            })
        }
    }
}

fn build_chunk_plan(project: &ProjectDocument, duration_ms: u64) -> Vec<ChunkPlan> {
    let mut boundaries = vec![0, duration_ms];

    for track in &project.timeline_tracks {
        for region in &track.regions {
            boundaries.push(region.start_ms.min(duration_ms));
            boundaries.push(region.end_ms.min(duration_ms));
        }
    }

    boundaries.sort_unstable();
    boundaries.dedup();

    let mut chunks = Vec::new();

    for window in boundaries.windows(2) {
        let start_ms = window[0];
        let end_ms = window[1];

        if end_ms <= start_ms {
            continue;
        }

        let sample_ms = start_ms + ((end_ms - start_ms) / 2);

        if active_region(project, TimelineTrackKind::Trim, sample_ms).is_some() {
            continue;
        }

        let speed = active_region(project, TimelineTrackKind::Speed, sample_ms)
            .and_then(|region| region.emphasis)
            .filter(|value| *value > 0.05)
            .unwrap_or(1.0);

        let magnify = active_region(project, TimelineTrackKind::Zoom, sample_ms)
            .and_then(|region| region.emphasis)
            .filter(|value| *value > 1.0);

        let focus_rect = active_region(project, TimelineTrackKind::Zoom, sample_ms)
            .and_then(|region| region.focus_rect);

        let text = active_region(project, TimelineTrackKind::Annotation, sample_ms)
            .map(|region| region.label.trim().to_string())
            .filter(|label| !label.is_empty());

        chunks.push(ChunkPlan {
            start_ms,
            end_ms,
            speed,
            magnify,
            focus_rect,
            text,
        });
    }

    chunks
}

fn active_region(project: &ProjectDocument, kind: TimelineTrackKind, sample_ms: u64) -> Option<&TimelineRegion> {
    project
        .timeline_tracks
        .iter()
        .find(|track| track.kind == kind)
        .and_then(|track| {
            track.regions.iter().find(|region| {
                sample_ms >= region.start_ms && sample_ms < region.end_ms && region.end_ms > region.start_ms
            })
        })
}

fn render_chunk(
    ffmpeg_path: &Path,
    input_path: &Path,
    output_path: &Path,
    chunk: &ChunkPlan,
    target_width: u32,
    target_height: u32,
) -> Result<(), String> {
    let mut filters = Vec::new();

    if let Some(magnify) = chunk.magnify {
        let safe_magnify = magnify.max(1.05);
        if let Some(focus_rect) = chunk.focus_rect {
            let x = focus_rect.x.clamp(0.0, 0.95);
            let y = focus_rect.y.clamp(0.0, 0.95);
            let width = focus_rect.width.clamp(0.05, 1.0 - x);
            let height = focus_rect.height.clamp(0.05, 1.0 - y);

            filters.push(format!(
                "crop=iw*{width:.6}:ih*{height:.6}:iw*{x:.6}:ih*{y:.6}"
            ));
        } else {
            filters.push(format!(
                "crop=iw/{safe_magnify:.4}:ih/{safe_magnify:.4}:(iw-iw/{safe_magnify:.4})/2:(ih-ih/{safe_magnify:.4})/2"
            ));
        }
    }

    filters.push(format!(
        "scale={target_width}:{target_height}:force_original_aspect_ratio=decrease,pad={target_width}:{target_height}:(ow-iw)/2:(oh-ih)/2:black"
    ));

    if let Some(text) = &chunk.text {
        filters.push(format!(
            "drawtext=text='{}':x=(w-text_w)/2:y=h-(text_h*2):fontcolor=white:fontsize=h/14:box=1:boxcolor=black@0.45:boxborderw=18",
            escape_drawtext(text)
        ));
    }

    let mut args = vec![
        String::from("-y"),
        String::from("-ss"),
        format_seconds(chunk.start_ms),
        String::from("-to"),
        format_seconds(chunk.end_ms),
        String::from("-i"),
        input_path.display().to_string(),
        String::from("-an"),
    ];

    if (chunk.speed - 1.0).abs() > f32::EPSILON {
        filters.push(format!("setpts=PTS/{:.6}", chunk.speed));
    }

    args.extend([
        String::from("-vf"),
        filters.join(","),
        String::from("-c:v"),
        String::from("libx264"),
        String::from("-preset"),
        String::from("veryfast"),
        String::from("-crf"),
        String::from("20"),
        String::from("-pix_fmt"),
        String::from("yuv420p"),
        String::from("-movflags"),
        String::from("+faststart"),
        output_path.display().to_string(),
    ]);

    run_ffmpeg(ffmpeg_path, args)
}

fn concat_chunks(
    ffmpeg_path: &Path,
    chunk_paths: &[PathBuf],
    output_path: &Path,
    temp_root: &Path,
) -> Result<(), String> {
    let list_path = temp_root.join("concat-list.txt");
    let mut list_file = String::new();

    for path in chunk_paths {
        list_file.push_str(&format!("file '{}'\n", path.display()));
    }

    fs::write(&list_path, list_file).map_err(|error| {
        format!(
            "failed to write the concat list at {}: {error}",
            list_path.display()
        )
    })?;

    run_ffmpeg(
        ffmpeg_path,
        [
            String::from("-y"),
            String::from("-f"),
            String::from("concat"),
            String::from("-safe"),
            String::from("0"),
            String::from("-i"),
            list_path.display().to_string(),
            String::from("-c"),
            String::from("copy"),
            output_path.display().to_string(),
        ],
    )
}

fn render_gif_from_mp4(
    ffmpeg_path: &Path,
    input_path: &Path,
    output_path: &Path,
    fps: u32,
    loop_gif: bool,
) -> Result<(), String> {
    let temp_dir = tempdir().map_err(|error| format!("failed to create a GIF temp directory: {error}"))?;
    let palette_path = temp_dir.path().join("palette.png");
    let fps = fps.max(1);

    run_ffmpeg(
        ffmpeg_path,
        [
            String::from("-y"),
            String::from("-i"),
            input_path.display().to_string(),
            String::from("-vf"),
            format!("fps={fps},palettegen=stats_mode=diff"),
            palette_path.display().to_string(),
        ],
    )?;

    run_ffmpeg(
        ffmpeg_path,
        [
            String::from("-y"),
            String::from("-i"),
            input_path.display().to_string(),
            String::from("-i"),
            palette_path.display().to_string(),
            String::from("-lavfi"),
            format!("fps={fps}[clip];[clip][1:v]paletteuse=dither=sierra2_4a"),
            String::from("-loop"),
            if loop_gif { String::from("0") } else { String::from("-1") },
            output_path.display().to_string(),
        ],
    )
}

fn probe_duration_ms(ffmpeg_path: &Path, input_path: &Path) -> Result<u64, String> {
    let ffprobe_path = ffmpeg_path.with_file_name("ffprobe");
    let output = Command::new(&ffprobe_path)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(input_path)
        .output()
        .map_err(|error| format!("failed to launch {}: {error}", ffprobe_path.display()))?;

    if !output.status.success() {
        return Err(format!(
            "{} exited with {} while probing duration: {}",
            ffprobe_path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let seconds = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("failed to parse media duration from {}: {error}", ffprobe_path.display()))?;

    Ok((seconds * 1000.0).round() as u64)
}

fn aspect_ratio_width(target_height: u32, aspect_ratio: &str) -> u32 {
    let (numerator, denominator) = match aspect_ratio {
        "9:16" => (9_u32, 16_u32),
        "1:1" => (1_u32, 1_u32),
        _ => (16_u32, 9_u32),
    };

    let width = ((target_height as f64) * (numerator as f64) / (denominator as f64)).round() as u32;

    if width % 2 == 0 { width.max(2) } else { (width + 1).max(2) }
}

fn format_seconds(value_ms: u64) -> String {
    format!("{:.3}", value_ms as f64 / 1000.0)
}

fn escape_drawtext(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
        .replace('%', "\\%")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn run_ffmpeg(ffmpeg_path: &Path, args: impl IntoIterator<Item = String>) -> Result<(), String> {
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

    Err(format!(
        "{} exited with {}: {}",
        ffmpeg_path.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::build_chunk_plan;
    use crate::session::{
        CompositeSettings, ExportFormat, ExportSettings, ProjectDocument, TimelineRegion,
        TimelineTrack, TimelineTrackKind,
    };

    fn project_with_regions(tracks: Vec<TimelineTrack>) -> ProjectDocument {
        ProjectDocument {
            version: 1,
            session_id: String::from("test-session"),
            created_at_unix_ms: 0,
            timeline_tracks: tracks,
            export: ExportSettings {
                primary_format: ExportFormat::Mp4,
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
            notes: Vec::new(),
        }
    }

    fn track(kind: TimelineTrackKind, regions: Vec<TimelineRegion>) -> TimelineTrack {
        TimelineTrack {
            id: format!("{:?}", kind),
            label: format!("{:?}", kind),
            kind,
            regions,
        }
    }

    fn region(label: &str, start_ms: u64, end_ms: u64, emphasis: Option<f32>) -> TimelineRegion {
        TimelineRegion {
            id: label.to_string(),
            label: label.to_string(),
            start_ms,
            end_ms,
            emphasis,
            focus_rect: None,
        }
    }

    #[test]
    fn trim_segments_are_removed_from_chunk_plan() {
        let project = project_with_regions(vec![track(
            TimelineTrackKind::Trim,
            vec![region("skip", 1_000, 2_000, None)],
        )]);

        let chunks = build_chunk_plan(&project, 4_000);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_ms, 0);
        assert_eq!(chunks[0].end_ms, 1_000);
        assert_eq!(chunks[1].start_ms, 2_000);
        assert_eq!(chunks[1].end_ms, 4_000);
    }

    #[test]
    fn speed_text_and_magnify_apply_to_matching_chunks() {
        let project = project_with_regions(vec![
            track(
                TimelineTrackKind::Speed,
                vec![region("Speed 1.5x", 500, 1_500, Some(1.5))],
            ),
            track(
                TimelineTrackKind::Zoom,
                vec![region("Magnify", 1_000, 2_000, Some(1.8))],
            ),
            track(
                TimelineTrackKind::Annotation,
                vec![region("Hello world", 1_000, 2_000, None)],
            ),
        ]);

        let chunks = build_chunk_plan(&project, 2_500);

        let target = chunks
            .iter()
            .find(|chunk| chunk.start_ms == 1_000 && chunk.end_ms == 1_500)
            .expect("expected a split chunk for the overlapping edited segment");

        assert_eq!(target.speed, 1.5);
        assert_eq!(target.magnify, Some(1.8));
        assert_eq!(target.text.as_deref(), Some("Hello world"));
    }
}