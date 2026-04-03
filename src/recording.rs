use crate::session::{self, CaptureArea, CaptureSourceKind, SessionManifest, SessionSource, SessionStage};

use ashpd::desktop::{
    PersistMode, Session,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};

use std::env;
use std::fs;
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct CaptureSource {
    pub id: String,
    pub name: String,
    pub kind: CaptureSourceKind,
    pub detail: String,
    pub capture_area: Option<CaptureArea>,
}

#[derive(Debug, Clone)]
pub struct RecordingRequest {
    pub source: CaptureSource,
    pub microphone_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedRecording {
    pub manifest: SessionManifest,
    pub backend_name: String,
    pub pipeline_summary: String,
}

#[derive(Debug)]
pub struct ActiveRecording {
    pub manifest: SessionManifest,
    pub backend_name: String,
    pub pipeline_summary: String,
    pub capture_target: PathBuf,
    child: Child,
    runtime: ActiveRecordingRuntime,
}

#[derive(Debug)]
enum ActiveRecordingRuntime {
    X11,
    WaylandPortal { _portal: WaylandPortalRecording },
}

#[derive(Debug)]
struct WaylandPortalRecording {
    _session: Session<Screencast>,
}

#[derive(Debug, Clone)]
pub enum BackendStatus {
    Available(String),
    Planned(String),
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct DependencyCheck {
    pub headline: String,
    pub capture_detail_lines: Vec<String>,
    pub packaging_detail_lines: Vec<String>,
    pub install_hint: Option<String>,
    pub packaging_hint: String,
    pub next_step_lines: Vec<String>,
}

pub trait RecordingBackend {
    fn display_name(&self) -> &'static str;
    fn status(&self) -> BackendStatus;
    fn enumerate_sources(&self) -> Result<Vec<CaptureSource>, String>;
    fn prepare_recording(&self, request: RecordingRequest) -> Result<PreparedRecording, String>;
    fn start_recording(&self, ffmpeg_path: &Path, prepared: PreparedRecording) -> Result<ActiveRecording, String>;
    fn stop_recording(&self, active: &mut ActiveRecording) -> Result<SessionManifest, String>;
}

pub fn backend_name<B: RecordingBackend>(backend: &B) -> &'static str {
    backend.display_name()
}

pub fn backend_status<B: RecordingBackend>(backend: &B) -> BackendStatus {
    backend.status()
}

pub fn enumerate_sources_with<B: RecordingBackend>(backend: B) -> Result<Vec<CaptureSource>, String> {
    backend.enumerate_sources()
}

pub fn prepare_recording_with<B: RecordingBackend>(backend: B, request: RecordingRequest) -> Result<PreparedRecording, String> {
    backend.prepare_recording(request)
}

pub fn start_recording_with<B: RecordingBackend>(backend: &B, ffmpeg_path: &Path, prepared: PreparedRecording) -> Result<ActiveRecording, String> {
    backend.start_recording(ffmpeg_path, prepared)
}

pub fn stop_recording_with<B: RecordingBackend>(backend: &B, active: &mut ActiveRecording) -> Result<SessionManifest, String> {
    backend.stop_recording(active)
}

pub fn inspect_linux_capture_dependencies() -> DependencyCheck {
    let session_type = if is_wayland_session() { "Wayland" } else { "X11 or non-Wayland" };
    let pkg_config_present = command_exists("pkg-config");
    let xrandr_present = command_exists("xrandr");
    let gstreamer_present = pkg_config_present && pkg_config_exists("gstreamer-1.0");
    let gst_base_present = pkg_config_present && pkg_config_exists("gstreamer-plugins-base-1.0");
    let pipewire_present = pkg_config_present && pkg_config_exists("libpipewire-0.3");
    let gst_launch_present = command_exists("gst-launch-1.0");
    let gst_inspect_present = command_exists("gst-inspect-1.0");
    let pipewiresrc_present = gst_inspect_present && gstreamer_element_exists("pipewiresrc");
    let flatpak_present = command_exists("flatpak");
    let flatpak_builder_present = command_exists("flatpak-builder");
    let appstreamcli_present = command_exists("appstreamcli");
    let desktop_file_validate_present = command_exists("desktop-file-validate");
    let portal_binary_present = command_exists("xdg-desktop-portal");
    let portal_service_present = portal_service_running();
    let portal_backends = installed_portal_backends();

    let mut capture_detail_lines = vec![format!("Session type: {session_type}")];
    capture_detail_lines.push(format!("pkg-config available: {}", yes_no(pkg_config_present)));
    capture_detail_lines.push(format!("xrandr available: {}", yes_no(xrandr_present)));
    capture_detail_lines.push(format!("gstreamer-1.0 dev files available: {}", yes_no(gstreamer_present)));
    capture_detail_lines.push(format!(
        "gstreamer-plugins-base-1.0 dev files available: {}",
        yes_no(gst_base_present)
    ));
    capture_detail_lines.push(format!("libpipewire-0.3 dev files available: {}", yes_no(pipewire_present)));
    capture_detail_lines.push(format!("gst-launch-1.0 available: {}", yes_no(gst_launch_present)));
    capture_detail_lines.push(format!("gst-inspect-1.0 available: {}", yes_no(gst_inspect_present)));
    capture_detail_lines.push(format!("pipewiresrc plugin available: {}", yes_no(pipewiresrc_present)));

    let wayland_dev_ready = gstreamer_present && gst_base_present && pipewire_present;
    let wayland_runtime_ready = gst_launch_present && gst_inspect_present && pipewiresrc_present;

    let packaging_detail_lines = vec![
        format!("flatpak available: {}", yes_no(flatpak_present)),
        format!("flatpak-builder available: {}", yes_no(flatpak_builder_present)),
        format!("appstreamcli available: {}", yes_no(appstreamcli_present)),
        format!(
            "desktop-file-validate available: {}",
            yes_no(desktop_file_validate_present)
        ),
        format!("xdg-desktop-portal binary available: {}", yes_no(portal_binary_present)),
        format!("portal D-Bus service reachable: {}", yes_no(portal_service_present)),
        format!(
            "installed portal backend files: {}",
            if portal_backends.is_empty() {
                String::from("none detected")
            } else {
                portal_backends.join(", ")
            }
        ),
    ];

    let headline = if is_wayland_session() {
        if wayland_dev_ready && wayland_runtime_ready {
            String::from(
                "Wayland capture prerequisites are present; the remaining blocker is implementing the PipeWire or portal-backed recorder flow in the app.",
            )
        } else if wayland_dev_ready {
            String::from(
                "Wayland development headers are present, but the GStreamer runtime tool or plugin layer is still incomplete on this machine.",
            )
        } else {
            String::from("Wayland capture prerequisites are incomplete on this machine, and the packaging toolchain is only partially installed.")
        }
    } else if xrandr_present {
        String::from("X11 fallback capture prerequisites are present through xrandr and bundled FFmpeg, while the Wayland and packaging paths still need host tools.")
    } else {
        String::from("The X11 fallback capture path is missing xrandr, and the Wayland stack is not ready.")
    };

    let install_hint = if is_wayland_session() && !(wayland_dev_ready && wayland_runtime_ready) {
        preferred_install_hint()
    } else if !is_wayland_session() && !xrandr_present {
        preferred_x11_install_hint()
    } else {
        None
    };

    let packaging_hint = String::from(
        "For shipping, prefer Flatpak or another app bundle that carries the GStreamer runtime and plugins while still relying on the host PipeWire service and desktop portal on Wayland. Flatpak does not replace host-installed development headers for native builds.",
    );

    let mut next_step_lines = Vec::new();

    if is_wayland_session() && !wayland_dev_ready {
        next_step_lines.push(String::from(
            "Install the Wayland capture development packages before implementing the real recorder backend.",
        ));
        next_step_lines.push(String::from(
            "Do not expect Flatpak runtime installation to satisfy native cargo build dependencies; those headers still need to exist on the host for a native backend build.",
        ));
    }

    if is_wayland_session() && wayland_dev_ready && !wayland_runtime_ready {
        next_step_lines.push(String::from(
            "Install the GStreamer runtime tools and the PipeWire GStreamer plugin before attempting a real Wayland capture pipeline.",
        ));
    }

    if !flatpak_builder_present || !appstreamcli_present || !desktop_file_validate_present {
        next_step_lines.push(String::from(
            "Install flatpak-builder, appstreamcli, and desktop-file-validate to validate the Flatpak and metadata flow locally.",
        ));
    }

    if is_wayland_session() && !portal_service_present {
        next_step_lines.push(String::from(
            "Start or install an xdg-desktop-portal service and a matching backend before attempting portal-based Wayland capture.",
        ));
    }

    if flatpak_present {
        next_step_lines.push(String::from(
            "Run ./flatpak/validate-packaging.sh once the missing validation tools are installed.",
        ));
    }

    if is_wayland_session() {
        next_step_lines.push(String::from(
            "Implement a portal or PipeWire-backed recording path after the host prerequisites are in place.",
        ));
    }

    DependencyCheck {
        headline,
        capture_detail_lines,
        packaging_detail_lines,
        install_hint,
        packaging_hint,
        next_step_lines,
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlannedGstreamerBackend;

impl PlannedGstreamerBackend {
    pub fn display_name(&self) -> &'static str {
        "Linux recorder backend"
    }

    pub fn status(&self) -> BackendStatus {
        if !cfg!(target_os = "linux") {
            BackendStatus::Unavailable(String::from(
                "The recorder backend currently targets Linux-first capture work.",
            ))
        } else if is_wayland_session() {
            let detail = if wayland_runtime_ready() {
                "Wayland session detected. A first-pass portal-backed recording path is available for monitor and window capture through PipeWire and gst-launch-1.0."
            } else if gstreamer_dev_available() {
                "Wayland session detected and GStreamer development libraries are available, but the runtime tool or plugin stack is still incomplete for live PipeWire capture."
            } else {
                "Wayland session detected, but gstreamer-1.0 development libraries are missing in this environment. Draft sessions and editor persistence are live, but real capture still needs PipeWire or GStreamer support."
            };

            if wayland_runtime_ready() {
                BackendStatus::Available(String::from(detail))
            } else {
                BackendStatus::Planned(String::from(detail))
            }
        } else if command_exists("xrandr") {
            BackendStatus::Available(String::from(
                "X11 session detected. Real display enumeration and an FFmpeg-based screen recording fallback are available for display capture.",
            ))
        } else {
            BackendStatus::Planned(String::from(
                "Linux session detected, but no display enumeration helper was found. Install xrandr or add a PipeWire/GStreamer backend for richer capture support.",
            ))
        }
    }

    pub fn enumerate_sources(&self) -> Result<Vec<CaptureSource>, String> {
        let session_hint = if is_wayland_session() {
            "Wayland session detected; final live capture will use PipeWire portals."
        } else {
            "Desktop session detected; display capture fallback is available through bundled FFmpeg on X11."
        };

        let mut sources = if !is_wayland_session() {
            enumerate_x11_displays()?
        } else {
            vec![CaptureSource {
                id: String::from("display-portal"),
                name: String::from("Desktop portal source"),
                kind: CaptureSourceKind::Display,
                detail: String::from("Placeholder display target for the future PipeWire portal picker."),
                capture_area: None,
            }]
        };

        if sources.is_empty() {
            sources.push(CaptureSource {
                id: String::from("display-primary"),
                name: String::from("Primary display"),
                kind: CaptureSourceKind::Display,
                detail: format!("Fallback display target. {session_hint}"),
                capture_area: None,
            });
        }

        sources.push(CaptureSource {
            id: String::from("window-focused"),
            name: String::from("Focused window"),
            kind: CaptureSourceKind::Window,
            detail: String::from("Editor-facing placeholder for a future single-window recording flow."),
            capture_area: None,
        });
        sources.push(CaptureSource {
            id: String::from("region-custom"),
            name: String::from("Custom region"),
            kind: CaptureSourceKind::Region,
            detail: String::from("Editor-facing placeholder for crop-based capture sessions."),
            capture_area: None,
        });

        Ok(sources)
    }

    pub fn prepare_recording(&self, request: RecordingRequest) -> Result<PreparedRecording, String> {
        let manifest = session::create_draft(SessionSource::from(&request.source), request.microphone_enabled)?;

        let pipeline_summary = if is_wayland_session() {
            format!(
                "Draft capture prepared for '{}' using the {}. This environment is Wayland-based, so the next start action will open the desktop portal picker and record the selected PipeWire stream with GStreamer.",
                request.source.name,
                self.display_name()
            )
        } else if request.source.kind == CaptureSourceKind::Display && request.source.capture_area.is_some() {
            format!(
                "Draft capture prepared for '{}' using the {}. This source can start an FFmpeg-based X11 recording immediately.",
                request.source.name,
                self.display_name()
            )
        } else {
            format!(
                "Draft capture prepared for '{}' using the {}. The session is persisted, but live capture for this source type is still pending.",
                request.source.name,
                self.display_name()
            )
        };

        Ok(PreparedRecording {
            manifest,
            backend_name: String::from(self.display_name()),
            pipeline_summary,
        })
    }

    pub fn start_recording(&self, ffmpeg_path: &Path, mut prepared: PreparedRecording) -> Result<ActiveRecording, String> {
        if is_wayland_session() {
            return start_wayland_recording(prepared);
        }

        let Some(area) = prepared.manifest.source.capture_area else {
            return Err(String::from(
                "Live recording currently supports display sources with known geometry only.",
            ));
        };

        if prepared.manifest.source.kind != CaptureSourceKind::Display {
            return Err(String::from(
                "Live recording currently supports full-display capture only.",
            ));
        }

        let display = env::var("DISPLAY").unwrap_or_else(|_| String::from(":0.0"));
        let capture_target = prepared.manifest.files.screen_capture_path.clone();

        session::update_stage(
            &mut prepared.manifest,
            SessionStage::Recording,
            format!(
                "Started X11 display capture at {}x{}+{},{} via bundled FFmpeg.",
                area.width, area.height, area.x, area.y
            ),
        )?;

        let mut command = Command::new(ffmpeg_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .args([
                "-y",
                "-f",
                "x11grab",
                "-framerate",
                "30",
                "-video_size",
                &format!("{}x{}", area.width, area.height),
                "-i",
                &format!("{}+{},{}", display, area.x, area.y),
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&capture_target);

        let child = command
            .spawn()
            .map_err(|error| format!("failed to start the bundled FFmpeg recorder: {error}"))?;

        Ok(ActiveRecording {
            manifest: prepared.manifest,
            backend_name: prepared.backend_name,
            pipeline_summary: prepared.pipeline_summary,
            capture_target,
            child,
            runtime: ActiveRecordingRuntime::X11,
        })
    }

    pub fn stop_recording(&self, active: &mut ActiveRecording) -> Result<SessionManifest, String> {
        match &active.runtime {
            ActiveRecordingRuntime::X11 => {
                if let Some(stdin) = active.child.stdin.as_mut() {
                    stdin
                        .write_all(b"q\n")
                        .map_err(|error| format!("failed to signal FFmpeg to stop recording cleanly: {error}"))?;
                }
            }
            ActiveRecordingRuntime::WaylandPortal { .. } => unsafe {
                if libc::kill(active.child.id() as i32, libc::SIGINT) != 0 {
                    return Err(String::from(
                        "failed to signal the GStreamer pipeline to stop recording cleanly.",
                    ));
                }
            },
        }

        let status = active
            .child
            .wait()
            .map_err(|error| format!("failed to wait for FFmpeg to stop: {error}"))?;

        if !status.success() {
            return Err(format!("FFmpeg exited unsuccessfully while stopping the recording: {status}"));
        }

        session::update_stage(
            &mut active.manifest,
            SessionStage::Editing,
            format!(
                "Stopped display capture. Recorded media is ready at {}.",
                active.capture_target.display()
            ),
        )?;

        let mut project = session::load_project(&active.manifest.files.project_path)?;
        session::append_project_note(
            &mut project,
            format!(
                "Recorded media saved to {}. The editor can now attach preview and timeline tools.",
                active.capture_target.display()
            ),
        )?;

        Ok(active.manifest.clone())
    }
}

impl RecordingBackend for PlannedGstreamerBackend {
    fn display_name(&self) -> &'static str {
        self.display_name()
    }

    fn status(&self) -> BackendStatus {
        self.status()
    }

    fn enumerate_sources(&self) -> Result<Vec<CaptureSource>, String> {
        self.enumerate_sources()
    }

    fn prepare_recording(&self, request: RecordingRequest) -> Result<PreparedRecording, String> {
        self.prepare_recording(request)
    }

    fn start_recording(&self, ffmpeg_path: &Path, prepared: PreparedRecording) -> Result<ActiveRecording, String> {
        self.start_recording(ffmpeg_path, prepared)
    }

    fn stop_recording(&self, active: &mut ActiveRecording) -> Result<SessionManifest, String> {
        self.stop_recording(active)
    }
}

impl From<&CaptureSource> for SessionSource {
    fn from(source: &CaptureSource) -> Self {
        Self {
            id: source.id.clone(),
            name: source.name.clone(),
            kind: source.kind,
            detail: source.detail.clone(),
            capture_area: source.capture_area,
        }
    }
}

fn enumerate_x11_displays() -> Result<Vec<CaptureSource>, String> {
    if !command_exists("xrandr") {
        return Ok(Vec::new());
    }

    let output = Command::new("xrandr")
        .arg("--listmonitors")
        .output()
        .map_err(|error| format!("failed to run xrandr --listmonitors: {error}"))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sources = Vec::new();

    for line in stdout.lines().skip(1) {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let Some(raw_geometry) = tokens.iter().find(|token| token.contains('x') && token.contains('+')) else {
            continue;
        };
        let Some(name) = tokens.last() else {
            continue;
        };
        let Some(area) = parse_monitor_geometry(raw_geometry) else {
            continue;
        };

        sources.push(CaptureSource {
            id: format!("display-{name}"),
            name: format!("Display {name}"),
            kind: CaptureSourceKind::Display,
            detail: format!("{}x{} at +{},{}.", area.width, area.height, area.x, area.y),
            capture_area: Some(area),
        });
    }

    Ok(sources)
}

fn parse_monitor_geometry(raw: &str) -> Option<CaptureArea> {
    let mut pieces = raw.split('+');
    let size = pieces.next()?;
    let x = pieces.next()?.parse::<i32>().ok()?;
    let y = pieces.next()?.parse::<i32>().ok()?;

    let mut size_parts = size.split('x');
    let width = size_parts.next()?.split('/').next()?.parse::<u32>().ok()?;
    let height = size_parts.next()?.split('/').next()?.parse::<u32>().ok()?;

    Some(CaptureArea { x, y, width, height })
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {command} >/dev/null 2>&1")])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn portal_service_running() -> bool {
    if command_exists("busctl") {
        return Command::new("busctl")
            .args(["--user", "status", "org.freedesktop.portal.Desktop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
    }

    if command_exists("dbus-send") {
        return Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.portal.Desktop",
                "--type=method_call",
                "--print-reply",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.DBus.Peer.Ping",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
    }

    false
}

fn installed_portal_backends() -> Vec<String> {
    let mut backends = Vec::new();
    let portal_dir = Path::new("/usr/share/xdg-desktop-portal/portals");

    let Ok(entries) = fs::read_dir(portal_dir) else {
        return backends;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.extension().and_then(|extension| extension.to_str()) != Some("portal") {
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            backends.push(stem.to_string());
        }
    }

    backends.sort();
    backends
}

fn gstreamer_dev_available() -> bool {
    Command::new("sh")
        .args(["-c", "command -v pkg-config >/dev/null 2>&1 && pkg-config --exists gstreamer-1.0"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn wayland_runtime_ready() -> bool {
    command_exists("gst-launch-1.0")
        && command_exists("gst-inspect-1.0")
        && gstreamer_element_exists("pipewiresrc")
        && portal_service_running()
}

fn pkg_config_exists(package: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v pkg-config >/dev/null 2>&1 && pkg-config --exists {package}")])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn gstreamer_element_exists(element: &str) -> bool {
    Command::new("gst-inspect-1.0")
        .arg(element)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn start_wayland_recording(mut prepared: PreparedRecording) -> Result<ActiveRecording, String> {
    if !wayland_runtime_ready() {
        return Err(String::from(
            "Wayland recording still needs gst-launch-1.0, gst-inspect-1.0, the pipewiresrc plugin, and a reachable desktop portal service.",
        ));
    }

    let portal = pollster::block_on(open_wayland_portal_stream(prepared.manifest.source.kind))?;
    let capture_target = prepared.manifest.files.screen_capture_path.clone();
    let node_id = portal.pipewire_node_id;

    session::update_stage(
        &mut prepared.manifest,
        SessionStage::Recording,
        format!(
            "Started Wayland portal capture for PipeWire node {}. Recording to {}.",
            node_id,
            capture_target.display()
        ),
    )?;

    let mut project = session::load_project(&prepared.manifest.files.project_path)?;
    session::append_project_note(
        &mut project,
        format!(
            "Wayland capture started through the desktop portal using PipeWire node {}.",
            node_id
        ),
    )?;

    let inherited_fd_number = 3;
    let pipewire_fd = portal.pipewire_fd.as_raw_fd();
    let capture_target_arg = capture_target.display().to_string();

    let mut command = Command::new("gst-launch-1.0");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("-e")
        .arg("pipewiresrc")
        .arg(format!("fd={inherited_fd_number}"))
        .arg(format!("path={node_id}"))
        .arg("do-timestamp=true")
        .arg("!")
        .arg("videoconvert")
        .arg("!")
        .arg("queue")
        .arg("!")
        .arg("x264enc")
        .arg("speed-preset=ultrafast")
        .arg("tune=zerolatency")
        .arg("!")
        .arg("mp4mux")
        .arg("faststart=true")
        .arg("!")
        .arg("filesink")
        .arg(format!("location={capture_target_arg}"));

    unsafe {
        command.pre_exec(move || {
            if libc::dup2(pipewire_fd, inherited_fd_number) == -1 {
                return Err(std::io::Error::last_os_error());
            }

            Ok(())
        });
    }

    let child = command
        .spawn()
        .map_err(|error| format!("failed to start the Wayland GStreamer recorder: {error}"))?;

    Ok(ActiveRecording {
        manifest: prepared.manifest,
        backend_name: prepared.backend_name,
        pipeline_summary: format!(
            "Wayland portal capture started for PipeWire node {} and recording to {}.",
            node_id,
            capture_target.display()
        ),
        capture_target,
        child,
        runtime: ActiveRecordingRuntime::WaylandPortal {
            _portal: WaylandPortalRecording {
                _session: portal.session,
            },
        },
    })
}

struct WaylandPortalStart {
    session: Session<Screencast>,
    pipewire_fd: OwnedFd,
    pipewire_node_id: u32,
}

async fn open_wayland_portal_stream(source_kind: CaptureSourceKind) -> Result<WaylandPortalStart, String> {
    let proxy = Screencast::new()
        .await
        .map_err(|error| format!("failed to connect to the screen cast portal: {error}"))?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|error| format!("failed to create the screen cast portal session: {error}"))?;

    let source_types = match source_kind {
        CaptureSourceKind::Display => Some(SourceType::Monitor.into()),
        CaptureSourceKind::Window => Some(SourceType::Window.into()),
        CaptureSourceKind::Region => {
            return Err(String::from(
                "Wayland region capture is not wired yet. Use the display or focused-window source for now.",
            ))
        }
    };

    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(source_types)
                .set_multiple(false)
                .set_restore_token(None)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|error| format!("failed to configure the screen cast session: {error}"))?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|error| format!("failed to start the portal screen cast session: {error}"))?
        .response()
        .map_err(|error| format!("the portal screen cast request was rejected: {error}"))?;

    let stream = response
        .streams()
        .first()
        .ok_or_else(|| String::from("the portal session did not return any selected stream"))?
        .to_owned();

    let pipewire_fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(|error| format!("failed to open the PipeWire remote for the screen cast session: {error}"))?;

    Ok(WaylandPortalStart {
        session,
        pipewire_fd,
        pipewire_node_id: stream.pipe_wire_node_id(),
    })
}

fn preferred_install_hint() -> Option<String> {
    if command_exists("apt-get") {
        Some(String::from(
            "Install the Wayland capture prerequisites with: sudo apt-get update && sudo apt-get install -y libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libpipewire-0.3-dev gstreamer1.0-tools gstreamer1.0-pipewire xdg-desktop-portal xdg-desktop-portal-gtk",
        ))
    } else if command_exists("dnf") {
        Some(String::from(
            "Install the Wayland capture prerequisites with: sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel pipewire-devel gstreamer1 gstreamer1-plugins-bad-free pipewire xdg-desktop-portal xdg-desktop-portal-gtk",
        ))
    } else if command_exists("pacman") {
        Some(String::from(
            "Install the Wayland capture prerequisites with: sudo pacman -S gstreamer gst-plugins-base-libs gst-plugin-pipewire pipewire xdg-desktop-portal xdg-desktop-portal-gtk",
        ))
    } else {
        None
    }
}

fn preferred_x11_install_hint() -> Option<String> {
    if command_exists("apt-get") {
        Some(String::from("Install xrandr with: sudo apt-get update && sudo apt-get install -y x11-xserver-utils"))
    } else if command_exists("dnf") {
        Some(String::from("Install xrandr with: sudo dnf install xrandr"))
    } else if command_exists("pacman") {
        Some(String::from("Install xrandr with: sudo pacman -S xorg-xrandr"))
    } else {
        None
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn is_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}