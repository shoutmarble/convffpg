mod conversion;
mod editor_export;
mod ffmpeg;
mod recording;
mod session;

use crate::conversion::{ConversionJob, ConversionOutcome, DetectionOutcome};
use crate::editor_export::EditorExportOutcome;
use crate::ffmpeg::BundledFfmpeg;
use crate::recording::{
    backend_name, backend_status, enumerate_sources_with, prepare_recording_with,
    start_recording_with, stop_recording_with, ActiveRecording, BackendStatus, CaptureSource,
    DependencyCheck, PlannedGstreamerBackend, PreparedRecording, RecordingRequest,
};
use crate::session::{
    LoadedSession, NormalizedRect, ProjectDocument, RecentSessionSummary, SessionManifest,
    TimelineRegion, TimelineTrackKind,
};

use iced::event::{self, Event};
use iced::widget::{button, column, container, image, row, slider, text, text_input};
use iced::{window, Alignment, Element, Fill, Subscription, Task, Theme};
use rfd::AsyncFileDialog;

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> iced::Result {
    iced::application(Convffpg::boot, Convffpg::update, Convffpg::view)
        .subscription(Convffpg::subscription)
        .theme(Convffpg::theme)
        .title(Convffpg::title)
        .run()
}

#[derive(Debug, Default)]
struct Convffpg {
    workspace: Workspace,
    bundled_ffmpeg: Option<BundledFfmpeg>,
    hovered_file: Option<PathBuf>,
    current_input: Option<PathBuf>,
    planned_output: Option<PathBuf>,
    last_output: Option<PathBuf>,
    ffmpeg_status: String,
    conversion_status: String,
    is_preparing_ffmpeg: bool,
    is_converting: bool,
    progress: ConversionProgress,
    last_error: Option<String>,
    recording_backend: PlannedGstreamerBackend,
    capture_sources: Vec<CaptureSource>,
    selected_capture_source_id: Option<String>,
    microphone_enabled: bool,
    recorder_status: String,
    recorder_error: Option<String>,
    prepared_recording: Option<PreparedRecording>,
    active_recording: Option<ActiveRecording>,
    recent_sessions: Vec<RecentSessionSummary>,
    editor_session: Option<SessionManifest>,
    editor_project: Option<ProjectDocument>,
    editor_status: String,
    editor_error: Option<String>,
    editor_note_input: String,
    editor_preview_image: Option<PathBuf>,
    editor_preview_status: String,
    editor_preview_time_ms: u64,
    editor_preview_duration_ms: u64,
    editor_track_kind: TimelineTrackKind,
    editor_selected_region_index: Option<usize>,
    editor_region_label_input: String,
    editor_region_start_input: String,
    editor_region_end_input: String,
    editor_region_emphasis_input: String,
    editor_focus_x_input: String,
    editor_focus_y_input: String,
    editor_focus_width_input: String,
    editor_focus_height_input: String,
    editor_export_in_progress: bool,
    editor_last_export: Option<PathBuf>,
    dependency_check: Option<DependencyCheck>,
}

#[derive(Debug, Clone)]
enum Message {
    OpenWorkspace(Workspace),
    BundledFfmpegReady(Result<BundledFfmpeg, String>),
    FileHovered(PathBuf),
    FileDropped(PathBuf),
    FilesHoveredLeft,
    PickMp4,
    Mp4Picked(Option<PathBuf>),
    FramesDetected(Result<DetectionOutcome, String>),
    RetryBundledFfmpeg,
    ConversionFinished(Result<ConversionOutcome, String>),
    OpenOutputFolder,
    OutputFolderOpened(Result<(), String>),
    RefreshCaptureSources,
    CaptureSourcesLoaded(Result<Vec<CaptureSource>, String>),
    RefreshDependencyCheck,
    DependencyCheckLoaded(DependencyCheck),
    SelectCaptureSource(String),
    ToggleMicrophone,
    PrepareRecordingDraft,
    RecordingDraftPrepared(Result<PreparedRecording, String>),
    StartRecording,
    StopRecording,
    RefreshRecentSessions,
    RecentSessionsLoaded(Result<Vec<RecentSessionSummary>, String>),
    OpenRecentSession(PathBuf),
    RecentSessionOpened(Result<LoadedSession, String>),
    EditorNoteChanged(String),
    SaveEditorNote,
    AddTimelineRegion(TimelineTrackKind),
    RemoveTimelineRegion(TimelineTrackKind),
    SetPrimaryExportFormat(session::ExportFormat),
    SetGifFpsPreset(u32),
    SetAspectRatio(String),
    ToggleCursorHighlight,
    SetBackgroundStyle(String),
    SetWebcamLayout(String),
    SetTargetHeight(u32),
    OpenSessionFolder,
    ExportEditorProject,
    EditorExportFinished(Result<EditorExportOutcome, String>),
    EditorPreviewScrubbed(u64),
    EditorDurationLoaded(Result<Option<u64>, String>),
    EditorPreviewGenerated(Result<Option<PathBuf>, String>),
    SelectEditorTrack(TimelineTrackKind),
    SelectTimelineRegion { kind: TimelineTrackKind, index: usize },
    EditorRegionLabelChanged(String),
    EditorRegionStartChanged(String),
    EditorRegionEndChanged(String),
    EditorRegionEmphasisChanged(String),
    EditorFocusXChanged(String),
    EditorFocusYChanged(String),
    EditorFocusWidthChanged(String),
    EditorFocusHeightChanged(String),
    LoadRegionPreset {
        kind: TimelineTrackKind,
        label: String,
        emphasis: Option<f32>,
    },
    AddConfiguredRegion,
    UpdateLastConfiguredRegion,
    NudgeSelectedRegion(i64),
    ResizeSelectedRegion(i64),
    SetSelectedRegionStart(u64),
    SetSelectedRegionEnd(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Workspace {
    #[default]
    Converter,
    Recorder,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ConversionProgress {
    #[default]
    Idle,
    DetectingFrames,
    ConvertingFrames,
    Finished,
    Failed,
}

impl Default for TimelineTrackKind {
    fn default() -> Self {
        Self::Zoom
    }
}

impl Convffpg {
    fn boot() -> (Self, Task<Message>) {
        let recording_backend = PlannedGstreamerBackend;

        (
            Self {
                ffmpeg_status: String::from(
                    "Preparing bundled FFmpeg. The embedded archive is unpacked on first launch.",
                ),
                conversion_status: String::from("Drop an MP4 anywhere in this window to start."),
                recorder_status: String::from(
                    "Recorder MVP scaffold ready. Refresh sources to create a persisted draft session.",
                ),
                editor_status: String::from("Open a session to edit its timeline and export settings."),
                editor_preview_status: String::from("Open a session to generate a preview thumbnail."),
                is_preparing_ffmpeg: true,
                progress: ConversionProgress::Idle,
                recording_backend: recording_backend.clone(),
                ..Self::default()
            },
            Task::batch([
                Self::prepare_bundled_ffmpeg(),
                Self::refresh_capture_sources(recording_backend),
                Self::refresh_recent_sessions(),
                Self::refresh_dependency_check(),
            ]),
        )
    }

    fn title(&self) -> String {
        String::from("convffpg")
    }

    fn theme(&self) -> Theme {
        Theme::TokyoNight
    }

    fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _status, _window| match event {
            Event::Window(window::Event::FileHovered(path)) => Some(Message::FileHovered(path)),
            Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
            Event::Window(window::Event::FilesHoveredLeft) => Some(Message::FilesHoveredLeft),
            _ => None,
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenWorkspace(workspace) => {
                self.workspace = workspace;
                Task::none()
            }
            Message::RefreshRecentSessions => Self::refresh_recent_sessions(),
            Message::RefreshDependencyCheck => Self::refresh_dependency_check(),
            Message::RecentSessionsLoaded(result) => {
                match result {
                    Ok(sessions) => self.recent_sessions = sessions,
                    Err(error) => self.recorder_error = Some(error),
                }

                Task::none()
            }
            Message::DependencyCheckLoaded(check) => {
                self.dependency_check = Some(check);
                Task::none()
            }
            Message::OpenRecentSession(manifest_path) => {
                Task::perform(async move { session::open_session(&manifest_path) }, Message::RecentSessionOpened)
            }
            Message::RecentSessionOpened(result) => {
                match result {
                    Ok(loaded) => {
                        self.apply_loaded_session(loaded);
                        self.workspace = Workspace::Editor;
                        self.recorder_error = None;
                        self.editor_error = None;

                        return Task::batch([self.refresh_editor_preview(), self.refresh_editor_duration()]);
                    }
                    Err(error) => self.recorder_error = Some(error),
                }

                Task::none()
            }
            Message::BundledFfmpegReady(result) => {
                self.is_preparing_ffmpeg = false;

                match result {
                    Ok(bundle) => {
                        self.ffmpeg_status = format!("Bundled FFmpeg ready: {}", bundle.binary_path.display());
                        self.last_error = None;
                        self.bundled_ffmpeg = Some(bundle);

                        return Task::batch([self.refresh_editor_preview(), self.refresh_editor_duration()]);
                    }
                    Err(error) => {
                        self.ffmpeg_status = String::from(
                            "Bundled FFmpeg could not be prepared. Retry the setup after fixing the issue.",
                        );
                        self.last_error = Some(error);
                        self.bundled_ffmpeg = None;
                    }
                }

                Task::none()
            }
            Message::FileHovered(path) => {
                if self.workspace != Workspace::Converter {
                    return Task::none();
                }

                self.hovered_file = Some(path);
                Task::none()
            }
            Message::FilesHoveredLeft => {
                if self.workspace != Workspace::Converter {
                    return Task::none();
                }

                self.hovered_file = None;
                Task::none()
            }
            Message::FileDropped(path) => {
                if self.workspace != Workspace::Converter {
                    return Task::none();
                }

                self.start_conversion(path)
            }
            Message::PickMp4 => {
                if self.is_converting {
                    self.conversion_status = String::from(
                        "A conversion is already running. Wait for it to finish before selecting another file.",
                    );
                    return Task::none();
                }

                Task::perform(pick_mp4_file(), Message::Mp4Picked)
            }
            Message::Mp4Picked(path) => {
                let Some(path) = path else {
                    self.conversion_status = String::from("No MP4 selected.");
                    return Task::none();
                };

                self.start_conversion(path)
            }
            Message::FramesDetected(result) => match result {
                Ok(detection) => {
                    self.progress = ConversionProgress::ConvertingFrames;
                    self.planned_output = Some(detection.output_path.clone());
                    self.last_error = None;
                    self.conversion_status = match detection.detected_frames {
                        Some(frame_count) => format!(
                            "Detected {frame_count} frames. Converting frames into {}...",
                            display_name(&detection.output_path)
                        ),
                        None => format!(
                            "Frame scan complete. Converting frames into {}...",
                            display_name(&detection.output_path)
                        ),
                    };

                    Task::perform(async move { conversion::render_gif(detection) }, Message::ConversionFinished)
                }
                Err(error) => {
                    self.is_converting = false;
                    self.progress = ConversionProgress::Failed;
                    self.last_error = Some(error);
                    self.conversion_status =
                        String::from("Frame detection failed. Review the error details below.");
                    Task::none()
                }
            },
            Message::RetryBundledFfmpeg => {
                if self.is_preparing_ffmpeg {
                    return Task::none();
                }

                self.ffmpeg_status = String::from("Retrying bundled FFmpeg setup...");
                self.is_preparing_ffmpeg = true;
                self.last_error = None;

                Self::prepare_bundled_ffmpeg()
            }
            Message::ConversionFinished(result) => {
                self.is_converting = false;

                match result {
                    Ok(outcome) => {
                        self.current_input = Some(outcome.input_path.clone());
                        self.planned_output = Some(outcome.output_path.clone());
                        self.last_output = Some(outcome.output_path.clone());
                        self.progress = ConversionProgress::Finished;
                        self.last_error = None;
                        self.conversion_status = match outcome.detected_frames {
                            Some(frame_count) => format!(
                                "GIF created successfully from {frame_count} detected frames: {}",
                                outcome.output_path.display()
                            ),
                            None => format!("GIF created successfully: {}", outcome.output_path.display()),
                        };
                    }
                    Err(error) => {
                        self.progress = ConversionProgress::Failed;
                        self.last_error = Some(error);
                        self.conversion_status =
                            String::from("Conversion failed. Review the error details below.");
                    }
                }

                Task::none()
            }
            Message::OpenOutputFolder => {
                let Some(path) = self.last_output.as_ref().or(self.planned_output.as_ref()) else {
                    return Task::none();
                };

                let folder = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));

                Task::perform(async move { open_output_folder(folder) }, Message::OutputFolderOpened)
            }
            Message::OutputFolderOpened(result) => {
                if let Err(error) = result {
                    self.last_error = Some(error);
                }

                Task::none()
            }
            Message::RefreshCaptureSources => {
                self.recorder_status = String::from("Refreshing capture sources...");
                self.recorder_error = None;
                Self::refresh_capture_sources(self.recording_backend.clone())
            }
            Message::CaptureSourcesLoaded(result) => {
                match result {
                    Ok(sources) => {
                        let retained_selection = self
                            .selected_capture_source_id
                            .as_ref()
                            .filter(|selected| sources.iter().any(|source| &source.id == *selected))
                            .cloned();

                        self.selected_capture_source_id = retained_selection
                            .or_else(|| sources.first().map(|source| source.id.clone()));
                        self.capture_sources = sources;
                        self.recorder_error = None;
                        self.recorder_status = format!(
                            "Loaded {} capture targets for the recorder backend.",
                            self.capture_sources.len()
                        );
                    }
                    Err(error) => {
                        self.capture_sources.clear();
                        self.selected_capture_source_id = None;
                        self.recorder_error = Some(error);
                        self.recorder_status = String::from("Capture source loading failed.");
                    }
                }

                Task::none()
            }
            Message::SelectCaptureSource(source_id) => {
                self.selected_capture_source_id = Some(source_id.clone());

                if let Some(source) = self.capture_sources.iter().find(|source| source.id == source_id) {
                    self.recorder_status =
                        format!("Selected '{}' for the next recording session.", source.name);
                }

                Task::none()
            }
            Message::ToggleMicrophone => {
                self.microphone_enabled = !self.microphone_enabled;
                self.recorder_status = if self.microphone_enabled {
                    String::from("Microphone capture is enabled for the next session.")
                } else {
                    String::from("Microphone capture is disabled for the next session.")
                };

                Task::none()
            }
            Message::PrepareRecordingDraft => {
                let Some(source) = self.selected_capture_source() else {
                    self.recorder_error =
                        Some(String::from("Select a capture target before preparing a draft session."));
                    return Task::none();
                };

                self.recorder_status =
                    format!("Preparing a draft recording session for '{}'...", source.name);
                self.recorder_error = None;

                let backend = self.recording_backend.clone();
                let request = RecordingRequest {
                    source,
                    microphone_enabled: self.microphone_enabled,
                };

                Task::perform(async move { prepare_recording_with(backend, request) }, Message::RecordingDraftPrepared)
            }
            Message::RecordingDraftPrepared(result) => match result {
                Ok(prepared) => {
                    self.recorder_status = prepared.pipeline_summary.clone();
                    self.recorder_error = None;

                    if let Ok(loaded) = session::open_session(&prepared.manifest.files.manifest_path) {
                        self.apply_loaded_session(loaded);
                    }

                    self.prepared_recording = Some(prepared);
                    self.workspace = Workspace::Editor;

                    return Task::batch([
                        Self::refresh_recent_sessions(),
                        self.refresh_editor_preview(),
                        self.refresh_editor_duration(),
                    ]);
                }
                Err(error) => {
                    self.recorder_status = String::from("Draft recording preparation failed.");
                    self.recorder_error = Some(error);
                    Task::none()
                }
            },
            Message::EditorNoteChanged(value) => {
                self.editor_note_input = value;
                Task::none()
            }
            Message::SelectEditorTrack(kind) => {
                self.editor_track_kind = kind;
                self.editor_selected_region_index = None;
                self.editor_focus_x_input.clear();
                self.editor_focus_y_input.clear();
                self.editor_focus_width_input.clear();
                self.editor_focus_height_input.clear();
                self.editor_status = format!("Editing the {} track.", track_kind_label(kind));
                Task::none()
            }
            Message::SelectTimelineRegion { kind, index } => {
                let Some(project) = &self.editor_project else {
                    self.editor_error = Some(String::from("Open a session before selecting a segment."));
                    return Task::none();
                };

                let Some(track) = project.timeline_tracks.iter().find(|track| track.kind == kind) else {
                    self.editor_error = Some(String::from("The selected track is unavailable."));
                    return Task::none();
                };

                let Some(region) = track.regions.get(index) else {
                    self.editor_error = Some(String::from("The selected segment no longer exists."));
                    return Task::none();
                };

                self.editor_track_kind = kind;
                self.editor_selected_region_index = Some(index);
                self.editor_region_label_input = region.label.clone();
                self.editor_region_start_input = region.start_ms.to_string();
                self.editor_region_end_input = region.end_ms.to_string();
                self.editor_region_emphasis_input = region
                    .emphasis
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_default();
                if let Some(focus_rect) = region.focus_rect {
                    self.editor_focus_x_input = format_percentage(focus_rect.x);
                    self.editor_focus_y_input = format_percentage(focus_rect.y);
                    self.editor_focus_width_input = format_percentage(focus_rect.width);
                    self.editor_focus_height_input = format_percentage(focus_rect.height);
                } else {
                    self.editor_focus_x_input.clear();
                    self.editor_focus_y_input.clear();
                    self.editor_focus_width_input.clear();
                    self.editor_focus_height_input.clear();
                }
                self.editor_error = None;
                self.editor_status = format!(
                    "Selected {} segment {}.",
                    track_kind_label(kind),
                    index + 1
                );
                self.editor_preview_time_ms = region.start_ms;
                self.refresh_editor_preview()
            }
            Message::EditorRegionLabelChanged(value) => {
                self.editor_region_label_input = value;
                Task::none()
            }
            Message::EditorRegionStartChanged(value) => {
                self.editor_region_start_input = value;
                Task::none()
            }
            Message::EditorRegionEndChanged(value) => {
                self.editor_region_end_input = value;
                Task::none()
            }
            Message::EditorRegionEmphasisChanged(value) => {
                self.editor_region_emphasis_input = value;
                Task::none()
            }
            Message::EditorFocusXChanged(value) => {
                self.editor_focus_x_input = value;
                Task::none()
            }
            Message::EditorFocusYChanged(value) => {
                self.editor_focus_y_input = value;
                Task::none()
            }
            Message::EditorFocusWidthChanged(value) => {
                self.editor_focus_width_input = value;
                Task::none()
            }
            Message::EditorFocusHeightChanged(value) => {
                self.editor_focus_height_input = value;
                Task::none()
            }
            Message::LoadRegionPreset {
                kind,
                label,
                emphasis,
            } => {
                self.editor_track_kind = kind;
                self.editor_selected_region_index = None;
                self.editor_region_label_input = label;

                if self.editor_region_start_input.trim().is_empty() {
                    self.editor_region_start_input = String::from("0");
                }

                if self.editor_region_end_input.trim().is_empty() {
                    self.editor_region_end_input = String::from("3000");
                }

                self.editor_region_emphasis_input = emphasis
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_default();
                if kind == TimelineTrackKind::Zoom {
                    self.editor_focus_x_input = String::from("25");
                    self.editor_focus_y_input = String::from("25");
                    self.editor_focus_width_input = String::from("50");
                    self.editor_focus_height_input = String::from("50");
                } else {
                    self.editor_focus_x_input.clear();
                    self.editor_focus_y_input.clear();
                    self.editor_focus_width_input.clear();
                    self.editor_focus_height_input.clear();
                }
                self.editor_status = format!("Primed the {} controls.", track_kind_label(kind));
                self.editor_error = None;
                Task::none()
            }
            Message::SaveEditorNote => {
                let note = self.editor_note_input.trim();

                if note.is_empty() {
                    self.editor_error = Some(String::from("Enter a note before saving it to the project."));
                    return Task::none();
                }

                let note = note.to_string();

                match self.mutate_editor_project(|project| {
                    project.notes.push(note.clone());
                    Ok(format!("Saved project note: {note}"))
                }) {
                    Ok(()) => self.editor_note_input.clear(),
                    Err(error) => self.editor_error = Some(error),
                }

                Task::none()
            }
            Message::AddConfiguredRegion => {
                let region_input = match self.editor_region_input() {
                    Ok(region_input) => region_input,
                    Err(error) => {
                        self.editor_error = Some(error);
                        return Task::none();
                    }
                };

                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == region_input.kind)
                        .ok_or_else(|| format!("Could not find the {} track.", track_kind_label(region_input.kind)))?;

                    let next_index = track.regions.len() + 1;
                    track.regions.push(TimelineRegion {
                        id: format!("{}-region-{}", track.id, next_index),
                        label: region_input.label.clone(),
                        start_ms: region_input.start_ms,
                        end_ms: region_input.end_ms,
                        emphasis: region_input.emphasis,
                        focus_rect: region_input.focus_rect,
                    });

                    Ok(format!(
                        "Added '{}' to the {} track.",
                        region_input.label,
                        track_kind_label(region_input.kind)
                    ))
                }) {
                    self.editor_error = Some(error);
                } else if let Some(project) = &self.editor_project {
                    if let Some(track) = project.timeline_tracks.iter().find(|track| track.kind == region_input.kind) {
                        self.editor_selected_region_index = track.regions.len().checked_sub(1);
                    }
                }

                Task::none()
            }
            Message::UpdateLastConfiguredRegion => {
                let region_input = match self.editor_region_input() {
                    Ok(region_input) => region_input,
                    Err(error) => {
                        self.editor_error = Some(error);
                        return Task::none();
                    }
                };

                let selected_index = self.editor_selected_region_index;

                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == region_input.kind)
                        .ok_or_else(|| format!("Could not find the {} track.", track_kind_label(region_input.kind)))?;

                    let selected_index = selected_index
                        .filter(|index| *index < track.regions.len())
                        .unwrap_or_else(|| track.regions.len().saturating_sub(1));

                    let last_region = track
                        .regions
                        .get_mut(selected_index)
                        .ok_or_else(|| format!("The {} track has no region to update.", track.label))?;

                    last_region.label = region_input.label.clone();
                    last_region.start_ms = region_input.start_ms;
                    last_region.end_ms = region_input.end_ms;
                    last_region.emphasis = region_input.emphasis;
                    last_region.focus_rect = region_input.focus_rect;

                    Ok(format!(
                        "Updated the last region on the {} track.",
                        track_kind_label(region_input.kind)
                    ))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::SetSelectedRegionStart(start_ms) => {
                let Some(selected_index) = self.editor_selected_region_index else {
                    self.editor_error = Some(String::from("Select a segment before adjusting its start handle."));
                    return Task::none();
                };

                let track_kind = self.editor_track_kind;
                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == track_kind)
                        .ok_or_else(|| format!("Could not find the {} track.", track_kind_label(track_kind)))?;
                    let region = track
                        .regions
                        .get_mut(selected_index)
                        .ok_or_else(|| String::from("The selected segment no longer exists."))?;

                    region.start_ms = start_ms.min(region.end_ms.saturating_sub(250));
                    Ok(format!("Adjusted the start handle on the {} segment.", track_kind_label(track_kind)))
                }) {
                    self.editor_error = Some(error);
                    return Task::none();
                }

                self.editor_region_start_input = start_ms.to_string();
                self.editor_preview_time_ms = start_ms;
                self.refresh_editor_preview()
            }
            Message::SetSelectedRegionEnd(end_ms) => {
                let Some(selected_index) = self.editor_selected_region_index else {
                    self.editor_error = Some(String::from("Select a segment before adjusting its end handle."));
                    return Task::none();
                };

                let track_kind = self.editor_track_kind;
                let mut resolved_end = end_ms;
                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == track_kind)
                        .ok_or_else(|| format!("Could not find the {} track.", track_kind_label(track_kind)))?;
                    let region = track
                        .regions
                        .get_mut(selected_index)
                        .ok_or_else(|| String::from("The selected segment no longer exists."))?;

                    resolved_end = end_ms.max(region.start_ms.saturating_add(250));
                    region.end_ms = resolved_end;
                    Ok(format!("Adjusted the end handle on the {} segment.", track_kind_label(track_kind)))
                }) {
                    self.editor_error = Some(error);
                    return Task::none();
                }

                self.editor_region_end_input = resolved_end.to_string();
                Task::none()
            }
            Message::NudgeSelectedRegion(delta_ms) => {
                let Some(selected_index) = self.editor_selected_region_index else {
                    self.editor_error = Some(String::from("Select a segment before nudging it on the timeline."));
                    return Task::none();
                };

                let track_kind = self.editor_track_kind;
                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == track_kind)
                        .ok_or_else(|| format!("Could not find the {} track.", track_kind_label(track_kind)))?;
                    let region = track
                        .regions
                        .get_mut(selected_index)
                        .ok_or_else(|| String::from("The selected segment no longer exists."))?;

                    let duration = region.end_ms.saturating_sub(region.start_ms);
                    let next_start = if delta_ms.is_negative() {
                        region.start_ms.saturating_sub(delta_ms.unsigned_abs())
                    } else {
                        region.start_ms.saturating_add(delta_ms as u64)
                    };

                    region.start_ms = next_start;
                    region.end_ms = next_start.saturating_add(duration);

                    Ok(format!("Moved the {} segment on the timeline.", track_kind_label(track_kind)))
                }) {
                    self.editor_error = Some(error);
                } else {
                    if let (Ok(start), Ok(end)) = (
                        self.editor_region_start_input.parse::<u64>(),
                        self.editor_region_end_input.parse::<u64>(),
                    ) {
                        let duration = end.saturating_sub(start);
                        let next_start = if delta_ms.is_negative() {
                            start.saturating_sub(delta_ms.unsigned_abs())
                        } else {
                            start.saturating_add(delta_ms as u64)
                        };
                        self.editor_region_start_input = next_start.to_string();
                        self.editor_region_end_input = next_start.saturating_add(duration).to_string();
                        self.editor_preview_time_ms = next_start;
                        return self.refresh_editor_preview();
                    }
                }

                Task::none()
            }
            Message::ResizeSelectedRegion(delta_ms) => {
                let Some(selected_index) = self.editor_selected_region_index else {
                    self.editor_error = Some(String::from("Select a segment before resizing it."));
                    return Task::none();
                };

                let track_kind = self.editor_track_kind;
                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == track_kind)
                        .ok_or_else(|| format!("Could not find the {} track.", track_kind_label(track_kind)))?;
                    let region = track
                        .regions
                        .get_mut(selected_index)
                        .ok_or_else(|| String::from("The selected segment no longer exists."))?;

                    let min_duration = 250_u64;
                    let next_end = if delta_ms.is_negative() {
                        region.end_ms.saturating_sub(delta_ms.unsigned_abs())
                    } else {
                        region.end_ms.saturating_add(delta_ms as u64)
                    };

                    region.end_ms = next_end.max(region.start_ms.saturating_add(min_duration));

                    Ok(format!("Resized the {} segment.", track_kind_label(track_kind)))
                }) {
                    self.editor_error = Some(error);
                } else if let (Ok(start), Ok(end)) = (
                    self.editor_region_start_input.parse::<u64>(),
                    self.editor_region_end_input.parse::<u64>(),
                ) {
                    let next_end = if delta_ms.is_negative() {
                        end.saturating_sub(delta_ms.unsigned_abs())
                    } else {
                        end.saturating_add(delta_ms as u64)
                    };
                    self.editor_region_end_input = next_end.max(start.saturating_add(250)).to_string();
                }

                Task::none()
            }
            Message::AddTimelineRegion(kind) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == kind)
                        .ok_or_else(|| format!("Could not find the {:?} track.", kind))?;
                    let index = track.regions.len() as u64;
                    let start_ms = index * 5_000;
                    let end_ms = start_ms + 3_000;
                    let (label, emphasis) = default_region_details(kind, index + 1);

                    track.regions.push(TimelineRegion {
                        id: format!("{}-region-{}", track.id, index + 1),
                        label: label.clone(),
                        start_ms,
                        end_ms,
                        emphasis,
                        focus_rect: default_focus_rect(kind),
                    });

                    Ok(format!("Added {label}."))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::RemoveTimelineRegion(kind) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    let track = project
                        .timeline_tracks
                        .iter_mut()
                        .find(|track| track.kind == kind)
                        .ok_or_else(|| format!("Could not find the {:?} track.", kind))?;

                    let removed = track
                        .regions
                        .pop()
                        .ok_or_else(|| format!("The {} track has no regions to remove.", track.label))?;

                    Ok(format!("Removed {}.", removed.label))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::EditorPreviewScrubbed(value) => {
                self.editor_preview_time_ms = value;
                self.editor_status = format!("Scrubbing preview to {}.", format_ms_short(value));
                self.refresh_editor_preview()
            }
            Message::EditorDurationLoaded(result) => {
                match result {
                    Ok(Some(duration_ms)) => self.editor_preview_duration_ms = duration_ms,
                    Ok(None) => self.editor_preview_duration_ms = 0,
                    Err(error) => self.editor_error = Some(error),
                }

                Task::none()
            }
            Message::SetPrimaryExportFormat(format) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.export.primary_format = format;
                    Ok(format!("Primary export format is now {:?}.", project.export.primary_format))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::SetGifFpsPreset(fps) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.export.gif_fps = fps;
                    Ok(format!("GIF FPS set to {}.", project.export.gif_fps))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::SetAspectRatio(aspect_ratio) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.composite.aspect_ratio = aspect_ratio.clone();
                    Ok(format!("Aspect ratio set to {}.", project.composite.aspect_ratio))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::ToggleCursorHighlight => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.composite.cursor_highlight = !project.composite.cursor_highlight;
                    Ok(format!(
                        "Cursor highlight is now {}.",
                        if project.composite.cursor_highlight { "enabled" } else { "disabled" }
                    ))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::SetBackgroundStyle(background_style) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.composite.background_style = background_style.clone();
                    Ok(format!("Background style set to {}.", project.composite.background_style))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::SetWebcamLayout(webcam_layout) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.composite.webcam_layout = webcam_layout.clone();
                    Ok(format!("Webcam layout set to {}.", project.composite.webcam_layout))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::SetTargetHeight(target_height) => {
                if let Err(error) = self.mutate_editor_project(|project| {
                    project.export.target_height = target_height;
                    Ok(format!("Target height set to {}.", project.export.target_height))
                }) {
                    self.editor_error = Some(error);
                }

                Task::none()
            }
            Message::OpenSessionFolder => {
                let Some(session) = &self.editor_session else {
                    self.editor_error = Some(String::from("Open a session before revealing its folder."));
                    return Task::none();
                };

                let session_dir = session.files.session_dir.clone();

                Task::perform(
                    async move { open_output_folder(session_dir) },
                    |result| match result {
                        Ok(()) => Message::OutputFolderOpened(Ok(())),
                        Err(error) => Message::OutputFolderOpened(Err(error)),
                    },
                )
            }
            Message::ExportEditorProject => {
                if self.editor_export_in_progress {
                    return Task::none();
                }

                let Some(bundle) = &self.bundled_ffmpeg else {
                    self.editor_error = Some(String::from("Bundled FFmpeg is not ready yet, so export cannot start."));
                    return Task::none();
                };

                let Some(session) = &self.editor_session else {
                    self.editor_error = Some(String::from("Open a session before exporting it."));
                    return Task::none();
                };

                let Some(project) = &self.editor_project else {
                    self.editor_error = Some(String::from("The project document is unavailable, so export cannot start."));
                    return Task::none();
                };

                let ffmpeg_path = bundle.binary_path.clone();
                let session = session.clone();
                let project = project.clone();

                self.editor_export_in_progress = true;
                self.editor_error = None;
                self.editor_status = format!(
                    "Exporting {} with Trim, Speed, Text, and Magnify segments applied...",
                    match project.export.primary_format {
                        session::ExportFormat::Gif => "GIF",
                        session::ExportFormat::Mp4 => "MP4",
                    }
                );

                Task::perform(
                    async move { editor_export::export_project(ffmpeg_path, session, project) },
                    Message::EditorExportFinished,
                )
            }
            Message::EditorExportFinished(result) => {
                self.editor_export_in_progress = false;

                match result {
                    Ok(outcome) => {
                        self.editor_last_export = Some(outcome.output_path.clone());
                        self.last_output = Some(outcome.output_path.clone());
                        self.editor_status = format!(
                            "Editor export completed: {} (staged MP4: {})",
                            outcome.output_path.display(),
                            outcome.intermediate_mp4_path.display()
                        );
                        self.editor_error = None;
                    }
                    Err(error) => {
                        self.editor_error = Some(error);
                        self.editor_status = String::from("Editor export failed.");
                    }
                }

                Task::none()
            }
            Message::EditorPreviewGenerated(result) => {
                match result {
                    Ok(path) => {
                        self.editor_preview_image = path;
                        self.editor_preview_status = if self.editor_preview_image.is_some() {
                            format!("Thumbnail preview generated at {}.", format_ms_short(self.editor_preview_time_ms))
                        } else {
                            String::from("No session media exists yet, so no thumbnail could be generated.")
                        };
                        self.editor_error = None;
                    }
                    Err(error) => {
                        self.editor_preview_image = None;
                        self.editor_preview_status = String::from("Preview thumbnail generation failed.");
                        self.editor_error = Some(error);
                    }
                }

                Task::none()
            }
            Message::StartRecording => {
                if self.active_recording.is_some() {
                    self.recorder_error = Some(String::from("A recording is already running."));
                    return Task::none();
                }

                let Some(bundle) = self.bundled_ffmpeg.as_ref() else {
                    self.recorder_error =
                        Some(String::from("Bundled FFmpeg is not ready yet, so live recording cannot start."));
                    return Task::none();
                };

                let prepared = match self.prepare_or_reuse_recording() {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.recorder_status = String::from("Recording could not be started.");
                        self.recorder_error = Some(error);
                        return Task::none();
                    }
                };

                match start_recording_with(&self.recording_backend, &bundle.binary_path, prepared.clone()) {
                    Ok(active) => {
                        self.recorder_status = format!(
                            "Recording started for '{}'. Output file: {}",
                            active.manifest.source.name,
                            active.capture_target.display()
                        );
                        self.recorder_error = None;
                        self.editor_session = Some(active.manifest.clone());
                        self.prepared_recording = Some(prepared);
                        self.active_recording = Some(active);
                        Task::batch([
                            Self::refresh_recent_sessions(),
                            self.refresh_editor_preview(),
                            self.refresh_editor_duration(),
                        ])
                    }
                    Err(error) => {
                        self.recorder_status = String::from("Recording could not be started.");
                        self.recorder_error = Some(error);
                        Task::none()
                    }
                }
            }
            Message::StopRecording => {
                let Some(mut active) = self.active_recording.take() else {
                    self.recorder_error = Some(String::from("No recording is currently running."));
                    return Task::none();
                };

                match stop_recording_with(&self.recording_backend, &mut active) {
                    Ok(manifest) => {
                        self.recorder_status = format!(
                            "Recording stopped. Media saved to {}.",
                            manifest.files.screen_capture_path.display()
                        );
                        self.recorder_error = None;
                        self.prepared_recording = None;

                        if let Ok(loaded) = session::open_session(&manifest.files.manifest_path) {
                            self.apply_loaded_session(loaded);
                            self.workspace = Workspace::Editor;
                        }

                        Task::batch([
                            Self::refresh_recent_sessions(),
                            self.refresh_editor_preview(),
                            self.refresh_editor_duration(),
                        ])
                    }
                    Err(error) => {
                        self.recorder_status = String::from("Stopping the recording failed.");
                        self.recorder_error = Some(error);
                        self.active_recording = Some(active);
                        Task::none()
                    }
                }
            }
        }
    }

    fn start_conversion(&mut self, path: PathBuf) -> Task<Message> {
        self.hovered_file = None;

        if self.is_converting {
            self.conversion_status = String::from(
                "A conversion is already running. Wait for it to finish before dropping another file.",
            );
            return Task::none();
        }

        if !conversion::is_supported_input(&path) {
            self.current_input = Some(path);
            self.last_error = Some(String::from("Only .mp4 files are accepted."));
            self.progress = ConversionProgress::Failed;
            self.conversion_status = String::from("Drop a file with the .mp4 extension to create a GIF.");
            return Task::none();
        }

        let Some(bundle) = self.bundled_ffmpeg.clone() else {
            self.current_input = Some(path);
            self.conversion_status = if self.is_preparing_ffmpeg {
                String::from("Bundled FFmpeg is still unpacking. Drop the MP4 again in a moment.")
            } else {
                String::from("Bundled FFmpeg is unavailable, so conversion cannot start.")
            };
            return Task::none();
        };

        self.current_input = Some(path.clone());
        self.planned_output = None;
        self.last_output = None;
        self.last_error = None;
        self.is_converting = true;
        self.progress = ConversionProgress::DetectingFrames;
        self.conversion_status = format!("Detecting frames in {}...", display_name(&path));

        let job = ConversionJob::new(bundle.binary_path, path);

        Task::perform(async move { conversion::detect_frames(job) }, Message::FramesDetected)
    }

    fn view(&self) -> Element<'_, Message> {
        let body = match self.workspace {
            Workspace::Converter => self.converter_view(),
            Workspace::Recorder => self.recorder_view(),
            Workspace::Editor => self.editor_view(),
        };

        let tabs = row![
            button(self.workspace_label(Workspace::Converter))
                .on_press(Message::OpenWorkspace(Workspace::Converter)),
            button(self.workspace_label(Workspace::Recorder))
                .on_press(Message::OpenWorkspace(Workspace::Recorder)),
            button(self.workspace_label(Workspace::Editor)).on_press(Message::OpenWorkspace(Workspace::Editor)),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let mut layout = column![tabs].spacing(20).width(Fill);

        if let Some(active) = &self.active_recording {
            layout = layout.push(
                container(
                    column![
                        text("Recording In Progress").size(24),
                        text(format!(
                            "{} is being captured with {}.",
                            active.manifest.source.name,
                            active.backend_name
                        ))
                        .size(16),
                        text(format!("Output: {}", active.capture_target.display())).size(16),
                        text(active.pipeline_summary.clone()).size(15),
                        row![
                            button("Stop Recording").on_press(Message::StopRecording),
                            button("Open Recorder").on_press(Message::OpenWorkspace(Workspace::Recorder)),
                            button("Open Editor").on_press(Message::OpenWorkspace(Workspace::Editor)),
                        ]
                        .spacing(10),
                    ]
                    .spacing(8),
                ),
            );
        }

        layout = layout.push(body);

        container(layout)
            .padding(24)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn converter_view(&self) -> Element<'_, Message> {
        let wayland_session = is_wayland_session();

        let drop_copy = if self.is_converting {
            "Conversion is in progress."
        } else if self.is_preparing_ffmpeg {
            "Bundled FFmpeg is being unpacked for first use."
        } else if let Some(path) = &self.hovered_file {
            if conversion::is_supported_input(path) {
                "Release the MP4 to convert it into an animated GIF."
            } else {
                "That file is not an MP4."
            }
        } else if wayland_session {
            "Wayland blocks drag-and-drop here. Use the button below to choose an MP4."
        } else {
            "Drag an MP4 file onto this window."
        };

        let mut content = column![
            text("MP4 to Animated GIF").size(38),
            text(drop_copy).size(20),
            text(
                "This app embeds a static FFmpeg archive, unpacks it into its own folder on first run, and invokes that extracted binary for every conversion."
            )
            .size(16),
            button("Choose MP4 File").on_press(Message::PickMp4),
            text(format!("FFmpeg: {}", self.ffmpeg_status)).size(16),
            text(format!("Status: {}", self.conversion_status)).size(16),
        ]
        .spacing(12)
        .align_x(Alignment::Center)
        .width(Fill);

        content = content.push(self.progress_view());

        if let Some(path) = &self.hovered_file {
            content = content.push(text(format!("Hovered file: {}", path.display())).size(16));
        }

        if let Some(path) = &self.current_input {
            content = content.push(text(format!("Input: {}", path.display())).size(16));
        }

        if let Some(path) = &self.last_output {
            content = content.push(text(format!("Output: {}", path.display())).size(16));
        }

        if let Some(path) = self.last_output.as_ref().or(self.planned_output.as_ref()) {
            content = content.push(text(format!("Output file: {}", file_name_or_path(path))).size(16));
            content = content.push(text(format!("Output folder: {}", parent_display(path))).size(16));
            content = content.push(button("Open Output Folder").on_press(Message::OpenOutputFolder));
        }

        if let Some(bundle) = &self.bundled_ffmpeg {
            content = content.push(
                text(format!("Extracted FFmpeg folder: {}", bundle.install_dir.display())).size(16),
            );
        }

        if let Some(error) = &self.last_error {
            content = content.push(text(format!("Error: {error}")).size(16));
        }

        if self.bundled_ffmpeg.is_none() && !self.is_preparing_ffmpeg {
            content = content.push(button("Retry bundled FFmpeg setup").on_press(Message::RetryBundledFfmpeg));
        }

        container(content).into()
    }

    fn prepare_bundled_ffmpeg() -> Task<Message> {
        Task::perform(async { ffmpeg::ensure_ready() }, Message::BundledFfmpegReady)
    }

    fn refresh_capture_sources(backend: PlannedGstreamerBackend) -> Task<Message> {
        Task::perform(async move { enumerate_sources_with(backend) }, Message::CaptureSourcesLoaded)
    }

    fn refresh_recent_sessions() -> Task<Message> {
        Task::perform(async { session::list_recent_sessions(12) }, Message::RecentSessionsLoaded)
    }

    fn refresh_dependency_check() -> Task<Message> {
        Task::perform(async { recording::inspect_linux_capture_dependencies() }, Message::DependencyCheckLoaded)
    }

    fn recorder_view(&self) -> Element<'_, Message> {
        let backend_summary = match backend_status(&self.recording_backend) {
            BackendStatus::Available(summary)
            | BackendStatus::Planned(summary)
            | BackendStatus::Unavailable(summary) => summary,
        };

        let microphone_label = if self.microphone_enabled {
            "Microphone: on"
        } else {
            "Microphone: off"
        };

        let start_button = if self.active_recording.is_none() {
            button("Start Recording").on_press(Message::StartRecording)
        } else {
            button("Start Recording")
        };

        let stop_button = if self.active_recording.is_some() {
            button("Stop Recording").on_press(Message::StopRecording)
        } else {
            button("Stop Recording")
        };

        let sources = self.capture_sources.iter().fold(
            column![text("Capture Targets").size(24)].spacing(8),
            |column, source| {
                let selected = self
                    .selected_capture_source_id
                    .as_ref()
                    .is_some_and(|selected| selected == &source.id);
                let label = if selected {
                    format!("[selected] {}", source.name)
                } else {
                    source.name.clone()
                };

                column.push(
                    column![
                        button(text(label)).on_press(Message::SelectCaptureSource(source.id.clone())),
                        text(format!(
                            "{} ({:?}) | area: {}",
                            source.detail,
                            source.kind,
                            format_capture_area(source.capture_area)
                        ))
                        .size(15),
                    ]
                    .spacing(4),
                )
            },
        );

        let recent_sessions = if self.recent_sessions.is_empty() {
            column![text("Recent Sessions").size(24), text("No saved sessions found yet.").size(16)]
                .spacing(8)
        } else {
            self.recent_sessions.iter().fold(
                column![text("Recent Sessions").size(24)].spacing(8),
                |column, session| {
                    column.push(
                        column![
                            button(text(format!(
                                "{} [{}]",
                                session.session_id,
                                format_session_stage(session.stage)
                            )))
                            .on_press(Message::OpenRecentSession(session.manifest_path.clone())),
                            text(format!(
                                "Source: {} | Updated: {}",
                                session.source_name,
                                session.updated_at_unix_ms
                            ))
                            .size(15),
                        ]
                        .spacing(4),
                    )
                },
            )
        };

        let dependency_section: Element<'_, Message> = if let Some(check) = &self.dependency_check {
            let mut section = column![
                text("Host Readiness").size(24),
                text(check.headline.clone()).size(16),
                button("Refresh Dependency Check").on_press(Message::RefreshDependencyCheck),
                text("Capture prerequisites").size(18),
            ]
            .spacing(8);

            for line in &check.capture_detail_lines {
                section = section.push(text(line.clone()).size(15));
            }

            section = section.push(text("Packaging prerequisites").size(18));

            for line in &check.packaging_detail_lines {
                section = section.push(text(line.clone()).size(15));
            }

            if let Some(install_hint) = &check.install_hint {
                section = section.push(text(format!("Install hint: {install_hint}")).size(15));
            }

            section = section.push(text(format!("Packaging hint: {}", check.packaging_hint)).size(15));

            if !check.next_step_lines.is_empty() {
                section = section.push(text("Next steps").size(18));

                for line in &check.next_step_lines {
                    section = section.push(text(format!("- {line}")).size(15));
                }
            }

            container(section).into()
        } else {
            container(
                column![
                    text("Host Readiness").size(24),
                    button("Refresh Dependency Check").on_press(Message::RefreshDependencyCheck),
                    text("Dependency probe has not completed yet.").size(15),
                ]
                .spacing(8),
            )
            .into()
        };

        let mut content = column![
            text("Recorder MVP Scaffold").size(38),
            text("This view supports recent-session reload plus live recording on X11 through bundled FFmpeg and on Wayland through the desktop portal plus PipeWire/GStreamer.").size(18),
            text(format!("Backend: {}", backend_name(&self.recording_backend))).size(16),
            text(backend_summary).size(16),
            row![
                button("Refresh Sources").on_press(Message::RefreshCaptureSources),
                button("Refresh Recent Sessions").on_press(Message::RefreshRecentSessions),
                button(microphone_label).on_press(Message::ToggleMicrophone),
                button("Prepare Draft Session").on_press(Message::PrepareRecordingDraft),
                start_button,
                stop_button,
            ]
            .spacing(12),
            text(format!("Status: {}", self.recorder_status)).size(16),
            sources,
            dependency_section,
            recent_sessions,
        ]
        .spacing(12)
        .width(Fill);

        if let Some(prepared) = &self.prepared_recording {
            content = content.push(text(format!("Latest draft session: {}", prepared.manifest.session_id)).size(16));
            content = content.push(text(format!("Manifest: {}", prepared.manifest.files.manifest_path.display())).size(16));
        }

        if let Some(active) = &self.active_recording {
            content = content.push(text(format!("Active recording: {}", active.manifest.session_id)).size(16));
            content = content.push(text(format!("Capture target: {}", active.capture_target.display())).size(16));
            content = content.push(text(format!("Active backend: {}", active.backend_name)).size(16));
            content = content.push(text(format!("Pipeline: {}", active.pipeline_summary)).size(16));
        }

        if let Some(error) = &self.recorder_error {
            content = content.push(text(format!("Error: {error}")).size(16));
        }

        container(content).into()
    }

    fn editor_view(&self) -> Element<'_, Message> {
        let Some(session) = &self.editor_session else {
            return container(
                column![
                    text("Editor").size(38),
                    text("No session is open yet. Prepare one from the Recorder tab or reopen a recent session.").size(18),
                ]
                .spacing(12),
            )
            .into();
        };

        let Some(project) = &self.editor_project else {
            return container(
                column![
                    text("Editor").size(38),
                    text("The session manifest is loaded, but the project document is unavailable.").size(18),
                ]
                .spacing(12),
            )
            .into();
        };

        let prepared_backend_name = self
            .prepared_recording
            .as_ref()
            .map(|prepared| prepared.backend_name.as_str())
            .unwrap_or("unknown");

        let backend_summary = self
            .prepared_recording
            .as_ref()
            .map(|prepared| prepared.pipeline_summary.as_str())
            .unwrap_or("Loaded session from disk.");

        let preview_summary = media_preview_summary(&session.files.screen_capture_path);
        let current_format_is_gif = project.export.primary_format == session::ExportFormat::Gif;
        let selected_track_label = track_kind_label(self.editor_track_kind);
        let selected_segment_label = self
            .editor_selected_region_index
            .map(|index| format!("{} #{}", selected_track_label, index + 1))
            .unwrap_or_else(|| format!("{} (none selected)", selected_track_label));
        let selected_region_bounds = project
            .timeline_tracks
            .iter()
            .find(|track| track.kind == self.editor_track_kind)
            .and_then(|track| self.editor_selected_region_index.and_then(|index| track.regions.get(index)))
            .map(|region| (region.start_ms, region.end_ms));
        let preview_duration_slider = self.editor_preview_duration_ms.min(u32::MAX as u64) as u32;
        let preview_time_slider = self.editor_preview_time_ms.min(preview_duration_slider as u64) as u32;

        let preview_surface: Element<'_, Message> = if let Some(path) = &self.editor_preview_image {
            container(
                column![
                    image(path.clone()).width(720).height(405),
                    row![
                        text(format!("{} / {}", format_ms_short(self.editor_preview_time_ms), format_ms_short(self.editor_preview_duration_ms))).size(14),
                        text(format!("Aspect {}", project.composite.aspect_ratio)).size(14),
                        text(format!("Background {}", project.composite.background_style)).size(14),
                    ]
                    .spacing(14)
                    .align_y(Alignment::Center),
                ]
                .spacing(10),
            )
            .into()
        } else {
            container(
                column![
                    text("Preview Surface").size(24),
                    text(self.editor_preview_status.clone()).size(16),
                    text(format!("Preview summary: {preview_summary}")).size(15),
                ]
                .spacing(8)
                .width(Fill)
                .align_x(Alignment::Center),
            )
            .into()
        };

        let mut timeline_rows = column![].spacing(10).width(Fill);
        for track in &project.timeline_tracks {
            let mut region_row = row![
                button(text(track_button_label(track.kind, self.editor_track_kind)))
                    .on_press(Message::SelectEditorTrack(track.kind)),
                button("+").on_press(Message::AddTimelineRegion(track.kind)),
                button("-").on_press(Message::RemoveTimelineRegion(track.kind)),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            if track.regions.is_empty() {
                region_row = region_row.push(text("No regions yet").size(14));
            } else {
                for (index, region) in track.regions.iter().enumerate() {
                    let emphasis_suffix = region
                        .emphasis
                        .map(|value| format!(" | {:.2}", value))
                        .unwrap_or_default();
                    let chip_label = if self.editor_track_kind == track.kind
                        && self.editor_selected_region_index == Some(index)
                    {
                        format!(
                            "[selected] {} | {}-{}{}",
                            region.label,
                            format_ms_short(region.start_ms),
                            format_ms_short(region.end_ms),
                            emphasis_suffix
                        )
                    } else {
                        format!(
                            "{} | {}-{}{}",
                            region.label,
                            format_ms_short(region.start_ms),
                            format_ms_short(region.end_ms),
                            emphasis_suffix
                        )
                    };
                    region_row = region_row.push(
                        button(text(chip_label)).on_press(Message::SelectTimelineRegion {
                            kind: track.kind,
                            index,
                        }),
                    );
                }
            }

            timeline_rows = timeline_rows.push(container(region_row).width(Fill));
        }

        let project_notes = if project.notes.is_empty() {
            text("No project notes saved yet.").size(14)
        } else {
            text(project.notes.join(" | ")).size(14)
        };

        let mut zoom_row = row![].spacing(8);
        for (label, emphasis) in [("1.25x", 1.25), ("1.5x", 1.50), ("1.8x", 1.80), ("2.2x", 2.20)] {
            zoom_row = zoom_row.push(
                button(text(label)).on_press(Message::LoadRegionPreset {
                    kind: TimelineTrackKind::Zoom,
                    label: format!("Magnify {label}"),
                    emphasis: Some(emphasis),
                }),
            );
        }

        let mut speed_row = row![].spacing(8);
        for (label, emphasis) in [("0.5x", 0.50), ("0.75x", 0.75), ("1.25x", 1.25), ("1.5x", 1.50)] {
            speed_row = speed_row.push(
                button(text(label)).on_press(Message::LoadRegionPreset {
                    kind: TimelineTrackKind::Speed,
                    label: format!("Speed {label}"),
                    emphasis: Some(emphasis),
                }),
            );
        }

        let left_column = column![
            row![
                text("Editor").size(34),
                button("Open Session Folder").on_press(Message::OpenSessionFolder),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            text("The layout now follows the OpenScreen model more closely: a large preview canvas, a timeline strip beneath it, and editor settings kept in a dedicated right rail.").size(17),
            container(
                column![
                    text("Preview").size(24),
                    preview_surface,
                    slider(
                        0..=preview_duration_slider.max(1),
                        preview_time_slider.min(preview_duration_slider.max(1)),
                        |value| Message::EditorPreviewScrubbed(value as u64),
                    ),
                    row![
                        button("Open Media Folder").on_press(Message::OpenSessionFolder),
                        text(format!("Source: {}", session.source.name)).size(14),
                        text(format!("Stage: {}", format_session_stage(session.stage))).size(14),
                        text(format!("Backend: {prepared_backend_name}")).size(14),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                ]
                .spacing(10)
                .width(Fill),
            )
            .width(Fill),
            container(
                column![
                    row![
                        text("Timeline").size(24),
                        button("+ Magnify").on_press(Message::LoadRegionPreset {
                            kind: TimelineTrackKind::Zoom,
                            label: String::from("Magnify Focus"),
                            emphasis: Some(1.25),
                        }),
                        button("+ Trim").on_press(Message::LoadRegionPreset {
                            kind: TimelineTrackKind::Trim,
                            label: String::from("Skip Segment"),
                            emphasis: None,
                        }),
                        button("+ Text").on_press(Message::LoadRegionPreset {
                            kind: TimelineTrackKind::Annotation,
                            label: String::from("Text Overlay"),
                            emphasis: None,
                        }),
                        button("+ Speed").on_press(Message::LoadRegionPreset {
                            kind: TimelineTrackKind::Speed,
                            label: String::from("Speed Ramp"),
                            emphasis: Some(1.50),
                        }),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    text("Trim skips a segment, Speed changes playback rate in a segment, Text overlays text, and Magnify zooms to a smaller region for a segment.").size(14),
                    timeline_rows,
                ]
                .spacing(10)
                .width(Fill),
            )
            .width(Fill),
            text(format!("Editor status: {}", self.editor_status)).size(15),
            text(format!("Editor error: {}", self.editor_error.as_deref().unwrap_or("none"))).size(15),
            text(format!("Recorder summary: {backend_summary}")).size(14),
        ]
        .spacing(14)
        .width(Fill);

        let right_column = container(
            column![
                text("Settings").size(28),
                text("Magnify").size(20),
                zoom_row,
                text("Playback Speed").size(20),
                speed_row,
                text("Video Effects").size(20),
                row![
                    button(if project.composite.cursor_highlight {
                        "Cursor Highlight [on]"
                    } else {
                        "Cursor Highlight [off]"
                    })
                    .on_press(Message::ToggleCursorHighlight),
                    button(if project.composite.aspect_ratio == "16:9" {
                        "16:9 [active]"
                    } else {
                        "16:9"
                    })
                    .on_press(Message::SetAspectRatio(String::from("16:9"))),
                    button(if project.composite.aspect_ratio == "9:16" {
                        "9:16 [active]"
                    } else {
                        "9:16"
                    })
                    .on_press(Message::SetAspectRatio(String::from("9:16"))),
                    button(if project.composite.aspect_ratio == "1:1" {
                        "1:1 [active]"
                    } else {
                        "1:1"
                    })
                    .on_press(Message::SetAspectRatio(String::from("1:1"))),
                ]
                .spacing(8),
                text("Background").size(20),
                row![
                    button(if project.composite.background_style == "Solid" {
                        "Solid [active]"
                    } else {
                        "Solid"
                    })
                    .on_press(Message::SetBackgroundStyle(String::from("Solid"))),
                    button(if project.composite.background_style == "Sunset" {
                        "Sunset [active]"
                    } else {
                        "Sunset"
                    })
                    .on_press(Message::SetBackgroundStyle(String::from("Sunset"))),
                    button(if project.composite.background_style == "Aurora" {
                        "Aurora [active]"
                    } else {
                        "Aurora"
                    })
                    .on_press(Message::SetBackgroundStyle(String::from("Aurora"))),
                ]
                .spacing(8),
                text("Webcam Layout").size(20),
                row![
                    button(if project.composite.webcam_layout == "Off" {
                        "Off [active]"
                    } else {
                        "Off"
                    })
                    .on_press(Message::SetWebcamLayout(String::from("Off"))),
                    button(if project.composite.webcam_layout == "Picture in Picture" {
                        "Picture in Picture [active]"
                    } else {
                        "Picture in Picture"
                    })
                    .on_press(Message::SetWebcamLayout(String::from("Picture in Picture"))),
                    button(if project.composite.webcam_layout == "Vertical Stack" {
                        "Vertical Stack [active]"
                    } else {
                        "Vertical Stack"
                    })
                    .on_press(Message::SetWebcamLayout(String::from("Vertical Stack"))),
                ]
                .spacing(8),
                text("Export").size(20),
                row![
                    button(if current_format_is_gif { "GIF [active]" } else { "GIF" })
                        .on_press(Message::SetPrimaryExportFormat(session::ExportFormat::Gif)),
                    button(if current_format_is_gif { "MP4" } else { "MP4 [active]" })
                        .on_press(Message::SetPrimaryExportFormat(session::ExportFormat::Mp4)),
                ]
                .spacing(8),
                row![
                    button(if project.export.target_height == 720 { "Low [active]" } else { "Low" })
                        .on_press(Message::SetTargetHeight(720)),
                    button(if project.export.target_height == 1080 { "Medium [active]" } else { "Medium" })
                        .on_press(Message::SetTargetHeight(1080)),
                    button(if project.export.target_height == 1440 { "High [active]" } else { "High" })
                        .on_press(Message::SetTargetHeight(1440)),
                ]
                .spacing(8),
                row![
                    button(if project.export.gif_fps == 12 { "12 FPS [active]" } else { "12 FPS" })
                        .on_press(Message::SetGifFpsPreset(12)),
                    button(if project.export.gif_fps == 15 { "15 FPS [active]" } else { "15 FPS" })
                        .on_press(Message::SetGifFpsPreset(15)),
                    button(if project.export.gif_fps == 24 { "24 FPS [active]" } else { "24 FPS" })
                        .on_press(Message::SetGifFpsPreset(24)),
                    button(if project.export.gif_fps == 30 { "30 FPS [active]" } else { "30 FPS" })
                        .on_press(Message::SetGifFpsPreset(30)),
                ]
                .spacing(8),
                text(format!(
                    "Current export: {:?} | {} FPS | {}p",
                    project.export.primary_format,
                    project.export.gif_fps,
                    project.export.target_height
                ))
                .size(14),
                text("Selected Segment").size(20),
                text(format!("Selected segment: {selected_segment_label}")).size(14),
                if let Some((start_ms, end_ms)) = selected_region_bounds {
                    {
                        let start_slider = start_ms.min(u32::MAX as u64) as u32;
                        let min_end = start_ms.saturating_add(250);
                        let end_slider = end_ms.max(min_end).min(u32::MAX as u64) as u32;
                        let max_slider = self.editor_preview_duration_ms.max(min_end).min(u32::MAX as u64) as u32;

                    column![
                        text("Start Handle").size(14),
                        slider(0..=preview_duration_slider.max(1), start_slider, |value| {
                            Message::SetSelectedRegionStart(value as u64)
                        }),
                        text("End Handle").size(14),
                        slider(
                            (start_slider.saturating_add(250))..=max_slider.max(start_slider.saturating_add(250)),
                            end_slider.max(start_slider.saturating_add(250)),
                            |value| Message::SetSelectedRegionEnd(value as u64),
                        ),
                    ]
                    .spacing(6)
                    }
                } else {
                    column![text("Select a segment to reveal draggable-style start and end handles.").size(14)]
                        .spacing(6)
                },
                text_input(region_label_placeholder(self.editor_track_kind), &self.editor_region_label_input)
                    .on_input(Message::EditorRegionLabelChanged)
                    .padding(10)
                    .size(16),
                text_input("Start ms", &self.editor_region_start_input)
                    .on_input(Message::EditorRegionStartChanged)
                    .padding(10)
                    .size(16),
                text_input("End ms", &self.editor_region_end_input)
                    .on_input(Message::EditorRegionEndChanged)
                    .padding(10)
                    .size(16),
                text_input(emphasis_placeholder(self.editor_track_kind), &self.editor_region_emphasis_input)
                    .on_input(Message::EditorRegionEmphasisChanged)
                    .padding(10)
                    .size(16),
                if self.editor_track_kind == TimelineTrackKind::Zoom {
                    column![
                        text("Magnify Focus Box").size(18),
                        row![
                            text_input("X %", &self.editor_focus_x_input)
                                .on_input(Message::EditorFocusXChanged)
                                .padding(10)
                                .size(16),
                            text_input("Y %", &self.editor_focus_y_input)
                                .on_input(Message::EditorFocusYChanged)
                                .padding(10)
                                .size(16),
                        ]
                        .spacing(8),
                        row![
                            text_input("Width %", &self.editor_focus_width_input)
                                .on_input(Message::EditorFocusWidthChanged)
                                .padding(10)
                                .size(16),
                            text_input("Height %", &self.editor_focus_height_input)
                                .on_input(Message::EditorFocusHeightChanged)
                                .padding(10)
                                .size(16),
                        ]
                        .spacing(8),
                        text("These percentages define the video region that Magnify crops into before scaling it back up.").size(13),
                    ]
                    .spacing(8)
                } else {
                    column![].spacing(0)
                },
                row![
                    button("Add Region").on_press(Message::AddConfiguredRegion),
                    button("Update Selected / Last").on_press(Message::UpdateLastConfiguredRegion),
                ]
                .spacing(8),
                row![
                    button("Move -0.5s").on_press(Message::NudgeSelectedRegion(-500)),
                    button("Move +0.5s").on_press(Message::NudgeSelectedRegion(500)),
                ]
                .spacing(8),
                row![
                    button("Shorter").on_press(Message::ResizeSelectedRegion(-500)),
                    button("Longer").on_press(Message::ResizeSelectedRegion(500)),
                ]
                .spacing(8),
                text("Notes").size(20),
                text_input("Add a project note", &self.editor_note_input)
                    .on_input(Message::EditorNoteChanged)
                    .padding(10)
                    .size(16),
                button("Save Note").on_press(Message::SaveEditorNote),
                button(if self.editor_export_in_progress {
                    "Exporting..."
                } else if current_format_is_gif {
                    "Export GIF"
                } else {
                    "Export MP4"
                })
                .on_press_maybe((!self.editor_export_in_progress).then_some(Message::ExportEditorProject)),
                project_notes,
                text(
                    self.editor_last_export
                        .as_ref()
                        .map(|path| format!("Last export: {}", path.display()))
                        .unwrap_or_else(|| String::from("Last export: none")),
                )
                .size(14),
                text(format!("Capture area: {}", format_capture_area(session.source.capture_area))).size(14),
                text(format!("Session media: {}", session.files.screen_capture_path.display())).size(14),
                text(format!("Manifest: {}", session.files.manifest_path.display())).size(14),
            ]
            .spacing(12)
            .width(360),
        );

        container(row![left_column, right_column].spacing(20).width(Fill).align_y(Alignment::Start)).into()
    }

    fn progress_view(&self) -> Element<'_, Message> {
        let steps = [
            self.step_label(
                "1. Detect Frames",
                matches!(
                    self.progress,
                    ConversionProgress::DetectingFrames
                        | ConversionProgress::ConvertingFrames
                        | ConversionProgress::Finished
                ),
                self.progress == ConversionProgress::DetectingFrames,
            ),
            self.step_label(
                "2. Convert Frames",
                matches!(self.progress, ConversionProgress::ConvertingFrames | ConversionProgress::Finished),
                self.progress == ConversionProgress::ConvertingFrames,
            ),
            self.step_label("3. Export GIF", self.progress == ConversionProgress::Finished, false),
        ];

        column(steps).spacing(8).align_x(Alignment::Center).into()
    }

    fn step_label<'a>(&self, label: &'a str, done: bool, active: bool) -> Element<'a, Message> {
        let prefix = if done {
            "[done]"
        } else if active {
            "[active]"
        } else if self.progress == ConversionProgress::Failed {
            "[stopped]"
        } else {
            "[pending]"
        };

        row![text(prefix).size(16), text(label).size(16)]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }

    fn workspace_label(&self, workspace: Workspace) -> &'static str {
        match (self.workspace, workspace) {
            (Workspace::Converter, Workspace::Converter) if self.active_recording.is_some() => {
                "GIF Converter [active] [recording]"
            }
            (Workspace::Recorder, Workspace::Recorder) if self.active_recording.is_some() => {
                "Recorder [active] [recording]"
            }
            (Workspace::Editor, Workspace::Editor) if self.active_recording.is_some() => {
                "Editor [active] [recording]"
            }
            (Workspace::Converter, Workspace::Converter) => "GIF Converter [active]",
            (Workspace::Recorder, Workspace::Recorder) => "Recorder [active]",
            (Workspace::Editor, Workspace::Editor) => "Editor [active]",
            (_, Workspace::Converter) if self.active_recording.is_some() => "GIF Converter [recording]",
            (_, Workspace::Recorder) if self.active_recording.is_some() => "Recorder [recording]",
            (_, Workspace::Editor) if self.active_recording.is_some() => "Editor [recording]",
            (_, Workspace::Converter) => "GIF Converter",
            (_, Workspace::Recorder) => "Recorder",
            (_, Workspace::Editor) => "Editor",
        }
    }

    fn selected_capture_source(&self) -> Option<CaptureSource> {
        let selected_id = self.selected_capture_source_id.as_ref()?;

        self.capture_sources
            .iter()
            .find(|source| &source.id == selected_id)
            .cloned()
    }

    fn prepare_or_reuse_recording(&self) -> Result<PreparedRecording, String> {
        let source = self
            .selected_capture_source()
            .ok_or_else(|| String::from("Select a capture target before starting a recording."))?;

        if let Some(prepared) = &self.prepared_recording {
            if prepared.manifest.source.id == source.id
                && prepared.manifest.microphone_enabled == self.microphone_enabled
            {
                return Ok(prepared.clone());
            }
        }

        prepare_recording_with(
            self.recording_backend.clone(),
            RecordingRequest {
                source,
                microphone_enabled: self.microphone_enabled,
            },
        )
    }

    fn apply_loaded_session(&mut self, loaded: LoadedSession) {
        self.prepared_recording = Some(PreparedRecording {
            manifest: loaded.manifest.clone(),
            backend_name: String::from(backend_name(&self.recording_backend)),
            pipeline_summary: String::from("Loaded session from disk."),
        });
        self.editor_session = Some(loaded.manifest);
        self.editor_project = Some(loaded.project);
        self.editor_note_input.clear();
        self.editor_status = String::from("Loaded session and project document from disk.");
        self.editor_preview_image = None;
        self.editor_preview_status = String::from("Generating a thumbnail preview if media exists.");
        self.editor_preview_time_ms = 0;
        self.editor_preview_duration_ms = 0;
        self.editor_selected_region_index = None;
        self.editor_focus_x_input.clear();
        self.editor_focus_y_input.clear();
        self.editor_focus_width_input.clear();
        self.editor_focus_height_input.clear();
        self.editor_export_in_progress = false;
        self.editor_last_export = None;
        self.editor_error = None;
    }

    fn mutate_editor_project<F>(&mut self, mutate: F) -> Result<(), String>
    where
        F: FnOnce(&mut ProjectDocument) -> Result<String, String>,
    {
        let session = self
            .editor_session
            .as_mut()
            .ok_or_else(|| String::from("Open a session before editing its project."))?;
        let project = self
            .editor_project
            .as_mut()
            .ok_or_else(|| String::from("Open a project before editing it."))?;

        let status = mutate(project)?;
        session::save_project(project)?;
        session::save_manifest(session)?;
        self.editor_status = status;
        self.editor_error = None;
        Ok(())
    }

    fn editor_region_input(&self) -> Result<ConfiguredRegionInput, String> {
        let label = if self.editor_region_label_input.trim().is_empty() {
            format!("{} Region", track_kind_label(self.editor_track_kind))
        } else {
            self.editor_region_label_input.trim().to_string()
        };

        let start_ms = self
            .editor_region_start_input
            .trim()
            .parse::<u64>()
            .map_err(|_| String::from("Start ms must be a whole number."))?;
        let end_ms = self
            .editor_region_end_input
            .trim()
            .parse::<u64>()
            .map_err(|_| String::from("End ms must be a whole number."))?;

        if end_ms <= start_ms {
            return Err(String::from("End ms must be greater than start ms."));
        }

        let emphasis = if self.editor_region_emphasis_input.trim().is_empty() {
            None
        } else {
            Some(
                self.editor_region_emphasis_input
                    .trim()
                    .parse::<f32>()
                    .map_err(|_| String::from("Emphasis must be a decimal number."))?,
            )
        };

        let focus_rect = if self.editor_track_kind == TimelineTrackKind::Zoom {
            Some(parse_focus_rect(
                &self.editor_focus_x_input,
                &self.editor_focus_y_input,
                &self.editor_focus_width_input,
                &self.editor_focus_height_input,
            )?)
        } else {
            None
        };

        Ok(ConfiguredRegionInput {
            kind: self.editor_track_kind,
            label,
            start_ms,
            end_ms,
            emphasis,
            focus_rect,
        })
    }

    fn refresh_editor_preview(&self) -> Task<Message> {
        let Some(session) = &self.editor_session else {
            return Task::none();
        };
        let Some(bundle) = &self.bundled_ffmpeg else {
            return Task::none();
        };

        let input_path = session.files.screen_capture_path.clone();
        let preview_path = session.files.session_dir.join("preview.jpg");
        let ffmpeg_path = bundle.binary_path.clone();
        let preview_time_ms = self.editor_preview_time_ms;

        Task::perform(
            async move { generate_preview_thumbnail(ffmpeg_path, input_path, preview_path, preview_time_ms) },
            Message::EditorPreviewGenerated,
        )
    }

    fn refresh_editor_duration(&self) -> Task<Message> {
        let Some(session) = &self.editor_session else {
            return Task::none();
        };
        let Some(bundle) = &self.bundled_ffmpeg else {
            return Task::none();
        };

        let input_path = session.files.screen_capture_path.clone();
        let ffmpeg_path = bundle.binary_path.clone();

        Task::perform(
            async move { probe_media_duration_ms(ffmpeg_path, input_path) },
            Message::EditorDurationLoaded,
        )
    }
}

#[derive(Debug, Clone)]
struct ConfiguredRegionInput {
    kind: TimelineTrackKind,
    label: String,
    start_ms: u64,
    end_ms: u64,
    emphasis: Option<f32>,
    focus_rect: Option<NormalizedRect>,
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), String::from)
}

fn file_name_or_path(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .map_or_else(|| path.display().to_string(), String::from)
}

fn parent_display(path: &Path) -> String {
    path.parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_else(|| String::from("."))
}

fn format_capture_area(area: Option<session::CaptureArea>) -> String {
    area.map(|area| format!("{}x{} at +{},{}", area.width, area.height, area.x, area.y))
        .unwrap_or_else(|| String::from("unavailable"))
}

fn format_session_stage(stage: session::SessionStage) -> &'static str {
    match stage {
        session::SessionStage::Draft => "draft",
        session::SessionStage::Prepared => "prepared",
        session::SessionStage::Recording => "recording",
        session::SessionStage::Editing => "editing",
        session::SessionStage::Exporting => "exporting",
    }
}

fn track_kind_label(kind: TimelineTrackKind) -> &'static str {
    match kind {
        TimelineTrackKind::Trim => "Trim",
        TimelineTrackKind::Speed => "Speed",
        TimelineTrackKind::Zoom => "Magnify",
        TimelineTrackKind::Annotation => "Text",
    }
}

fn track_button_label(kind: TimelineTrackKind, active: TimelineTrackKind) -> String {
    if kind == active {
        format!("{} [editing]", track_kind_label(kind))
    } else {
        String::from(track_kind_label(kind))
    }
}

fn format_ms_short(value: u64) -> String {
    let total_seconds = value / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;

    format!("{minutes}:{seconds:02}")
}

fn region_label_placeholder(kind: TimelineTrackKind) -> &'static str {
    match kind {
        TimelineTrackKind::Trim => "Skip segment label",
        TimelineTrackKind::Speed => "Speed segment label",
        TimelineTrackKind::Zoom => "Magnify segment label",
        TimelineTrackKind::Annotation => "Text overlay content",
    }
}

fn emphasis_placeholder(kind: TimelineTrackKind) -> &'static str {
    match kind {
        TimelineTrackKind::Trim => "Unused for trim segments",
        TimelineTrackKind::Speed => "Playback speed, for example 1.50",
        TimelineTrackKind::Zoom => "Magnify level, for example 1.80",
        TimelineTrackKind::Annotation => "Unused for text overlays",
    }
}

fn default_region_details(kind: TimelineTrackKind, ordinal: u64) -> (String, Option<f32>) {
    match kind {
        TimelineTrackKind::Trim => (format!("Skip Segment {ordinal}"), None),
        TimelineTrackKind::Speed => (String::from("Speed 1.5x"), Some(1.5)),
        TimelineTrackKind::Zoom => (String::from("Magnify 1.25x"), Some(1.25)),
        TimelineTrackKind::Annotation => (format!("Text Overlay {ordinal}"), None),
    }
}

fn default_focus_rect(kind: TimelineTrackKind) -> Option<NormalizedRect> {
    match kind {
        TimelineTrackKind::Zoom => Some(NormalizedRect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        }),
        _ => None,
    }
}

fn parse_focus_rect(x: &str, y: &str, width: &str, height: &str) -> Result<NormalizedRect, String> {
    let x = parse_percentage(x, "Focus X")?;
    let y = parse_percentage(y, "Focus Y")?;
    let width = parse_percentage(width, "Focus width")?;
    let height = parse_percentage(height, "Focus height")?;

    if x + width > 1.0 {
        return Err(String::from("Focus X plus focus width must stay within 100%."));
    }

    if y + height > 1.0 {
        return Err(String::from("Focus Y plus focus height must stay within 100%."));
    }

    Ok(NormalizedRect { x, y, width, height })
}

fn parse_percentage(value: &str, label: &str) -> Result<f32, String> {
    let parsed = value
        .trim()
        .parse::<f32>()
        .map_err(|_| format!("{label} must be a percentage number."))?;

    if !(0.0..=100.0).contains(&parsed) {
        return Err(format!("{label} must be between 0 and 100."));
    }

    Ok(parsed / 100.0)
}

fn format_percentage(value: f32) -> String {
    format!("{:.0}", value * 100.0)
}

async fn pick_mp4_file() -> Option<PathBuf> {
    AsyncFileDialog::new()
        .set_title("Choose an MP4 to convert")
        .add_filter("MP4 video", &["mp4"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn open_output_folder(path: PathBuf) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to open {}: {error}", path.display()))
}

fn media_preview_summary(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(metadata) => format!(
            "media exists at {} with {} bytes",
            path.display(),
            metadata.len()
        ),
        Err(_) => format!(
            "no recorded media found yet at {}; start or finish a recording to populate the preview",
            path.display()
        ),
    }
}

fn probe_media_duration_ms(ffmpeg_path: PathBuf, input_path: PathBuf) -> Result<Option<u64>, String> {
    if !input_path.exists() {
        return Ok(None);
    }

    let ffprobe_path = ffmpeg_path.with_file_name("ffprobe");
    if !ffprobe_path.exists() {
        return Ok(None);
    }

    let output = Command::new(&ffprobe_path)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(&input_path)
        .output()
        .map_err(|error| format!("failed to launch {} for preview duration probing: {error}", ffprobe_path.display()))?;

    if !output.status.success() {
        return Ok(None);
    }

    let seconds = String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().ok();
    Ok(seconds.map(|seconds| (seconds * 1000.0).round() as u64))
}

fn generate_preview_thumbnail(
    ffmpeg_path: PathBuf,
    input_path: PathBuf,
    preview_path: PathBuf,
    preview_time_ms: u64,
) -> Result<Option<PathBuf>, String> {
    if !input_path.exists() {
        return Ok(None);
    }

    if let Some(parent) = preview_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create the preview directory at {}: {error}",
                parent.display()
            )
        })?;
    }

    let output = Command::new(&ffmpeg_path)
        .args([
            "-y",
            "-ss",
            &format!("{:.3}", preview_time_ms as f64 / 1000.0),
            "-i",
            &input_path.display().to_string(),
            "-vf",
            "scale=960:-1",
            "-frames:v",
            "1",
            &preview_path.display().to_string(),
        ])
        .output()
        .map_err(|error| format!("failed to launch {} for preview generation: {error}", ffmpeg_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "preview generation failed via {}: {}",
            ffmpeg_path.display(),
            stderr.trim()
        ));
    }

    Ok(Some(preview_path))
}
