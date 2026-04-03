import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

type TimelineTrackKind = "Trim" | "Speed" | "Zoom" | "Annotation";
type ExportFormat = "Gif" | "Mp4";

type NormalizedRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type TimelineRegion = {
  id: string;
  label: string;
  start_ms: number;
  end_ms: number;
  emphasis: number | null;
  focus_rect?: NormalizedRect | null;
};

type TimelineTrack = {
  id: string;
  label: string;
  kind: TimelineTrackKind;
  regions: TimelineRegion[];
};

type ExportSettings = {
  primary_format: ExportFormat;
  gif_fps: number;
  loop_gif: boolean;
  target_height: number;
};

type CompositeSettings = {
  aspect_ratio: string;
  background_style: string;
  webcam_layout: string;
  cursor_highlight: boolean;
};

type ProjectDocument = {
  version: number;
  session_id: string;
  created_at_unix_ms: number;
  timeline_tracks: TimelineTrack[];
  export: ExportSettings;
  composite: CompositeSettings;
  notes: string[];
};

type SessionFiles = {
  session_dir: string;
  manifest_path: string;
  project_path: string;
  screen_capture_path: string;
  webcam_capture_path: string | null;
  exported_gif_path: string;
  exported_mp4_path: string;
};

type SessionSource = {
  id: string;
  name: string;
  kind: string;
  detail: string;
  capture_area: unknown;
};

type SessionManifest = {
  version: number;
  session_id: string;
  created_at_unix_ms: number;
  stage: string;
  source: SessionSource;
  microphone_enabled: boolean;
  files: SessionFiles;
  notes: string[];
};

type LoadedSession = {
  manifest: SessionManifest;
  project: ProjectDocument;
};

type RecentSessionSummary = {
  session_id: string;
  manifest_path: string;
  source_name: string;
  stage: string;
  updated_at_unix_ms: number;
};

type SessionPayload = {
  manifest: SessionManifest;
  project: ProjectDocument;
  source_video_path: string;
};

type FfmpegStatus = {
  install_dir: string;
  binary_path: string;
};

type ExportOutcome = {
  output_path: string;
  intermediate_mp4_path: string;
};

type Selection = {
  trackIndex: number;
  regionId: string;
};

type TimelineDragState = {
  trackIndex: number;
  regionId: string;
  mode: "move" | "resize-start" | "resize-end";
  pointerStartX: number;
  startMs: number;
  endMs: number;
  durationMs: number;
  trackWidth: number;
};

type FocusDragState = {
  mode: "move" | "resize";
  pointerStartX: number;
  pointerStartY: number;
  rect: NormalizedRect;
  bounds: DOMRect;
};

type SnapGuide = {
  ms: number;
  label: string;
};

const TRACK_SEQUENCE: TimelineTrackKind[] = ["Trim", "Speed", "Zoom", "Annotation"];
const MIN_REGION_MS = 200;
const SNAP_MS = 250;

function App() {
  const [session, setSession] = useState<LoadedSession | null>(null);
  const [videoPath, setVideoPath] = useState<string | null>(null);
  const [videoDurationMs, setVideoDurationMs] = useState(30000);
  const [selected, setSelected] = useState<Selection | null>(null);
  const [recentSessions, setRecentSessions] = useState<RecentSessionSummary[]>([]);
  const [status, setStatus] = useState("Pick an MP4 to start the imported-video workflow.");
  const [ffmpegStatus, setFfmpegStatus] = useState<string>("Checking bundled FFmpeg...");
  const [busy, setBusy] = useState(false);
  const [lastExportPath, setLastExportPath] = useState<string | null>(null);
  const [playheadMs, setPlayheadMs] = useState(0);
  const [snapGuide, setSnapGuide] = useState<SnapGuide | null>(null);

  const videoRef = useRef<HTMLVideoElement | null>(null);
  const timelineDragRef = useRef<TimelineDragState | null>(null);
  const focusDragRef = useRef<FocusDragState | null>(null);
  const previewOverlayRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    void refreshRecentSessions();
    void checkFfmpeg();
  }, []);

  const selectedRegion = useMemo(() => {
    if (!session || !selected) {
      return null;
    }

    const track = session.project.timeline_tracks[selected.trackIndex];
    if (!track) {
      return null;
    }

    return track.regions.find((region) => region.id === selected.regionId) ?? null;
  }, [selected, session]);

  const selectedRegionIndex = useMemo(() => {
    if (!session || !selected) {
      return -1;
    }

    const track = session.project.timeline_tracks[selected.trackIndex];
    if (!track) {
      return -1;
    }

    return track.regions.findIndex((region) => region.id === selected.regionId);
  }, [selected, session]);

  const selectedTrackKind = useMemo(() => {
    if (!session || !selected) {
      return null;
    }

    return session.project.timeline_tracks[selected.trackIndex]?.kind ?? null;
  }, [selected, session]);

  const activeAnnotationRegion = useMemo(() => {
    if (!session) {
      return null;
    }

    return activeRegionForKind(session.project, "Annotation", playheadMs);
  }, [playheadMs, session]);

  const isEditingActiveAnnotation =
    selectedTrackKind === "Annotation"
    && selectedRegion !== null
    && activeAnnotationRegion !== null
    && selectedRegion.id === activeAnnotationRegion.id;

  useEffect(() => {
    function handlePointerMove(event: PointerEvent) {
      const timelineDrag = timelineDragRef.current;
      if (timelineDrag) {
        const deltaPixels = event.clientX - timelineDrag.pointerStartX;
        const deltaMs = Math.round((deltaPixels / Math.max(timelineDrag.trackWidth, 1)) * timelineDrag.durationMs);

        updateProject((draft) => {
          const track = draft.timeline_tracks[timelineDrag.trackIndex];
          const region = track?.regions.find((entry) => entry.id === timelineDrag.regionId);

          if (!track || !region) {
            return draft;
          }

          const sorted = [...track.regions].sort(compareRegions);
          const sortedIndex = sorted.findIndex((entry) => entry.id === timelineDrag.regionId);
          const previous = sortedIndex > 0 ? sorted[sortedIndex - 1] : null;
          const next = sortedIndex >= 0 && sortedIndex < sorted.length - 1 ? sorted[sortedIndex + 1] : null;
          const snapCandidates = collectSnapCandidates(
            draft.timeline_tracks,
            region.id,
            playheadMs,
            timelineDrag.durationMs
          );

          if (timelineDrag.mode === "move") {
            const lengthMs = timelineDrag.endMs - timelineDrag.startMs;
            const boundedStart = clamp(
              timelineDrag.startMs + deltaMs,
              previous?.end_ms ?? 0,
              Math.max((next?.start_ms ?? timelineDrag.durationMs) - lengthMs, 0)
            );
            const snappedMove = snapMoveBounds(boundedStart, lengthMs, snapCandidates, previous?.end_ms ?? 0, next?.start_ms ?? timelineDrag.durationMs);
            region.start_ms = snappedMove.startMs;
            region.end_ms = snappedMove.endMs;
            setSnapGuide(snappedMove.guide);
          } else if (timelineDrag.mode === "resize-start") {
            const nextStart = snapValue(
              clamp(
              timelineDrag.startMs + deltaMs,
              previous?.end_ms ?? 0,
              Math.max(timelineDrag.endMs - MIN_REGION_MS, 0)
              ),
              snapCandidates,
              "start"
            );
            region.start_ms = nextStart.value;
            setSnapGuide(nextStart.guide);
          } else {
            const nextEnd = snapValue(
              clamp(
              timelineDrag.endMs + deltaMs,
              Math.min(timelineDrag.startMs + MIN_REGION_MS, timelineDrag.durationMs),
              next?.start_ms ?? timelineDrag.durationMs
              ),
              snapCandidates,
              "end"
            );
            region.end_ms = nextEnd.value;
            setSnapGuide(nextEnd.guide);
          }

          return draft;
        }, { trackIndex: timelineDrag.trackIndex, regionId: timelineDrag.regionId });

        return;
      }

      const focusDrag = focusDragRef.current;
      if (focusDrag && selectedRegion?.focus_rect) {
        const deltaX = (event.clientX - focusDrag.pointerStartX) / Math.max(focusDrag.bounds.width, 1);
        const deltaY = (event.clientY - focusDrag.pointerStartY) / Math.max(focusDrag.bounds.height, 1);

        updateSelectedRegion((region) => {
          const current = region.focus_rect ?? focusDrag.rect;
          const next = { ...current };

          if (focusDrag.mode === "move") {
            next.x = clamp(focusDrag.rect.x + deltaX, 0, 1 - focusDrag.rect.width);
            next.y = clamp(focusDrag.rect.y + deltaY, 0, 1 - focusDrag.rect.height);
          } else {
            next.width = clamp(focusDrag.rect.width + deltaX, 0.08, 1 - focusDrag.rect.x);
            next.height = clamp(focusDrag.rect.height + deltaY, 0.08, 1 - focusDrag.rect.y);
          }

          return {
            ...region,
            focus_rect: next
          };
        });
      }
    }

    function handlePointerUp() {
      timelineDragRef.current = null;
      focusDragRef.current = null;
      setSnapGuide(null);
    }

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);

    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
    };
  }, [selectedRegion]);

  async function checkFfmpeg() {
    try {
      const result = await invoke<FfmpegStatus>("ffmpeg_status");
      setFfmpegStatus(result.binary_path);
    } catch (error) {
      setFfmpegStatus(String(error));
    }
  }

  async function refreshRecentSessions() {
    try {
      const results = await invoke<RecentSessionSummary[]>("list_recent_sessions", { limit: 8 });
      setRecentSessions(results);
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function pickMp4() {
    const picked = await open({
      multiple: false,
      filters: [{ name: "MP4 Video", extensions: ["mp4"] }]
    });

    if (!picked || Array.isArray(picked)) {
      return;
    }

    await importMp4(picked);
  }

  async function openExistingSession() {
    const picked = await open({
      multiple: false,
      filters: [{ name: "Session Manifest", extensions: ["json"] }]
    });

    if (!picked || Array.isArray(picked)) {
      return;
    }

    await loadSession(picked);
  }

  async function importMp4(sourcePath: string) {
    setBusy(true);
    setStatus("Importing MP4 into a session...");

    try {
      const payload = await invoke<SessionPayload>("import_mp4", { sourcePath });
      applyPayload(payload);
      setStatus(`Imported ${payload.manifest.source.name}.`);
      await refreshRecentSessions();
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function loadSession(manifestPath: string) {
    setBusy(true);
    setStatus("Opening session...");

    try {
      const payload = await invoke<SessionPayload>("open_session", { manifestPath });
      applyPayload(payload);
      setStatus(`Opened ${payload.manifest.source.name}.`);
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  function applyPayload(payload: SessionPayload) {
    setSession({ manifest: payload.manifest, project: payload.project });
    setVideoPath(convertFileSrc(payload.source_video_path));
    setLastExportPath(null);
    setPlayheadMs(0);

    const firstTrack = payload.project.timeline_tracks.find((track) => track.regions.length > 0);
    if (firstTrack) {
      setSelected({
        trackIndex: payload.project.timeline_tracks.indexOf(firstTrack),
        regionId: firstTrack.regions[0].id
      });
    } else {
      setSelected(null);
    }
  }

  function updateProject(mutator: (draft: ProjectDocument) => ProjectDocument, preferredSelection?: Selection | null) {
    let nextSelection: Selection | null = null;

    setSession((current) => {
      if (!current) {
        return current;
      }

      const nextProject = normalizeProjectDocument(mutator(structuredClone(current.project)), videoDurationMs);
      nextSelection = resolveSelection(nextProject, preferredSelection ?? selected);

      return {
        ...current,
        project: nextProject
      };
    });

    setSelected(nextSelection);
  }

  function ensureTrack(kind: TimelineTrackKind) {
    return session?.project.timeline_tracks.findIndex((track) => track.kind === kind) ?? -1;
  }

  function addRegion(kind: TimelineTrackKind) {
    if (!session) {
      return;
    }

    const trackIndex = ensureTrack(kind);
    if (trackIndex === -1) {
      return;
    }

    const startMs = Math.max(0, Math.min(playheadMs, videoDurationMs));
    const defaultLength = Math.max(1200, Math.floor(videoDurationMs * 0.1));
    const endMs = Math.min(videoDurationMs, startMs + defaultLength);

    const region: TimelineRegion = {
      id: `${kind.toLowerCase()}-${Date.now()}`,
      label: kind === "Annotation" ? "New text" : kind,
      start_ms: startMs,
      end_ms: Math.max(startMs + 500, endMs),
      emphasis: kind === "Speed" ? 1.5 : kind === "Zoom" ? 1.6 : null,
      focus_rect: kind === "Zoom"
        ? { x: 0.2, y: 0.2, width: 0.6, height: 0.6 }
        : null
    };

    updateProject((draft) => {
      draft.timeline_tracks[trackIndex].regions.push(region);
      return draft;
    }, { trackIndex, regionId: region.id });
  }

  function removeSelectedRegion() {
    if (!session || !selected) {
      return;
    }

    updateProject((draft) => {
      const track = draft.timeline_tracks[selected.trackIndex];
      const regionIndex = track.regions.findIndex((region) => region.id === selected.regionId);

      if (regionIndex >= 0) {
        track.regions.splice(regionIndex, 1);
      }

      return draft;
    });
  }

  function updateSelectedRegion(mutator: (region: TimelineRegion) => TimelineRegion) {
    if (!selected) {
      return;
    }

    updateProject((draft) => {
      const track = draft.timeline_tracks[selected.trackIndex];

      const regionIndex = track.regions.findIndex((region) => region.id === selected.regionId);
      if (regionIndex === -1) {
        return draft;
      }

      track.regions[regionIndex] = mutator(track.regions[regionIndex]);
      return draft;
    }, selected);
  }

  function beginTimelineDrag(
    event: React.PointerEvent<HTMLElement>,
    trackIndex: number,
    regionIndex: number,
    mode: TimelineDragState["mode"]
  ) {
    event.preventDefault();
    event.stopPropagation();

    const trackElement = event.currentTarget.closest(".timeline-track");
    if (!(trackElement instanceof HTMLElement) || !session) {
      return;
    }

    const region = session.project.timeline_tracks[trackIndex]?.regions[regionIndex];
    if (!region) {
      return;
    }

    timelineDragRef.current = {
      trackIndex,
      regionId: region.id,
      mode,
      pointerStartX: event.clientX,
      startMs: region.start_ms,
      endMs: region.end_ms,
      durationMs: Math.max(videoDurationMs, 1),
      trackWidth: trackElement.getBoundingClientRect().width
    };
  }

  function beginFocusDrag(event: React.PointerEvent<HTMLDivElement>, mode: FocusDragState["mode"]) {
    event.preventDefault();
    event.stopPropagation();

    if (!selectedRegion?.focus_rect || !previewOverlayRef.current) {
      return;
    }

    focusDragRef.current = {
      mode,
      pointerStartX: event.clientX,
      pointerStartY: event.clientY,
      rect: selectedRegion.focus_rect,
      bounds: previewOverlayRef.current.getBoundingClientRect()
    };
  }

  async function saveProject() {
    if (!session) {
      return;
    }

    setBusy(true);
    setStatus("Saving project...");

    try {
      await invoke("save_project_document", { project: session.project });
      setStatus("Project saved.");
      await refreshRecentSessions();
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function exportCurrentProject(format: ExportFormat) {
    if (!session) {
      return;
    }

    setBusy(true);
    setStatus(`Preparing ${format.toUpperCase()} export...`);

    try {
      const project: ProjectDocument = structuredClone(session.project);
      project.export.primary_format = format;
      await invoke("save_project_document", { project });
      setSession({ ...session, project });

      const result = await invoke<ExportOutcome>("export_project", {
        manifestPath: session.manifest.files.manifest_path
      });

      setLastExportPath(result.output_path);
      setStatus(`Export complete: ${result.output_path}`);
      await refreshRecentSessions();
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  const notesValue = session?.project.notes.join("\n") ?? "";

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">Tauri rewrite</p>
          <h1>convffpg editor</h1>
        </div>
        <div className="topbar-actions">
          <button className="ghost-button" onClick={() => void openExistingSession()} disabled={busy}>
            Open session
          </button>
          <button className="accent-button" onClick={() => void pickMp4()} disabled={busy}>
            Import MP4
          </button>
        </div>
      </header>

      <section className="status-strip">
        <div>
          <span className="status-label">Status</span>
          <strong>{status}</strong>
        </div>
        <div>
          <span className="status-label">Bundled FFmpeg</span>
          <strong>{ffmpegStatus}</strong>
        </div>
      </section>

      <main className="workspace-grid">
        <section className="editor-column">
          <div className="preview-panel card">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Preview</p>
                <h2>{session?.manifest.source.name ?? "Imported MP4"}</h2>
              </div>
              <div className="meta-chip">
                {session?.manifest.source.detail ?? "No session open"}
              </div>
            </div>

            <div className="video-stage">
              {videoPath ? (
                <div className="preview-frame">
                  <video
                    ref={videoRef}
                    src={videoPath}
                    controls
                    onLoadedMetadata={(event) => {
                      const nextDurationMs = Math.floor(event.currentTarget.duration * 1000);
                      if (Number.isFinite(nextDurationMs) && nextDurationMs > 0) {
                        setVideoDurationMs(nextDurationMs);
                      }
                    }}
                    onTimeUpdate={(event) => {
                      setPlayheadMs(Math.floor(event.currentTarget.currentTime * 1000));
                    }}
                  />
                  <div ref={previewOverlayRef} className="preview-overlay">
                    {selectedTrackKind === "Zoom" && selectedRegion?.focus_rect ? (
                      <div
                        className="focus-box"
                        style={{
                          left: `${selectedRegion.focus_rect.x * 100}%`,
                          top: `${selectedRegion.focus_rect.y * 100}%`,
                          width: `${selectedRegion.focus_rect.width * 100}%`,
                          height: `${selectedRegion.focus_rect.height * 100}%`
                        }}
                        onPointerDown={(event) => beginFocusDrag(event, "move")}
                      >
                        <span>Magnify focus</span>
                        <div
                          className="focus-resize-handle"
                          onPointerDown={(event) => beginFocusDrag(event, "resize")}
                        />
                      </div>
                    ) : null}
                    {activeAnnotationRegion?.label.trim() && !isEditingActiveAnnotation ? (
                      <div className="annotation-preview-chip">
                        <span>{activeAnnotationRegion.label}</span>
                      </div>
                    ) : null}
                    {selectedTrackKind === "Annotation" && selectedRegion ? (
                      <div className="annotation-editor-shell">
                        <textarea
                          className="annotation-editor"
                          value={selectedRegion.label}
                          onChange={(event) => {
                            const value = event.target.value;
                            updateSelectedRegion((region) => ({ ...region, label: value }));
                          }}
                          onPointerDown={(event) => event.stopPropagation()}
                          placeholder="Type overlay text"
                        />
                      </div>
                    ) : null}
                  </div>
                </div>
              ) : (
                <div className="empty-stage">
                  <p>Import an MP4 to start editing in the Tauri shell.</p>
                </div>
              )}
            </div>

            <div className="scrub-row">
              <label htmlFor="scrub">Playhead</label>
              <input
                id="scrub"
                type="range"
                min={0}
                max={Math.max(videoDurationMs, 1)}
                value={Math.min(playheadMs, videoDurationMs)}
                onChange={(event) => {
                  const next = Number(event.target.value);
                  setPlayheadMs(next);
                  if (videoRef.current) {
                    videoRef.current.currentTime = next / 1000;
                  }
                }}
              />
              <span>{formatMs(playheadMs)} / {formatMs(videoDurationMs)}</span>
            </div>
          </div>

          <div className="timeline-panel card">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Timeline</p>
                <h2>Imported-media workflow</h2>
              </div>
              <div className="toolbar-row timeline-toolbar">
                <span className="timeline-note">Snaps to 0.25s against the playhead and all track edges.</span>
                {TRACK_SEQUENCE.map((kind) => (
                  <button key={kind} className="ghost-button small" onClick={() => addRegion(kind)} disabled={!session || busy}>
                    Add {trackDisplayName(kind)}
                  </button>
                ))}
              </div>
            </div>

            <div className="timeline-grid">
              {(session?.project.timeline_tracks ?? []).map((track, trackIndex) => (
                <div key={track.id} className="timeline-row">
                  <div className="timeline-label">
                    <span>{track.label}</span>
                    <small>{track.regions.length} segments</small>
                  </div>
                  <div className="timeline-track">
                    <div className="timeline-ruler" />
                    {snapGuide ? (
                      <div
                        className="timeline-snap-guide"
                        style={{ left: `${(snapGuide.ms / Math.max(videoDurationMs, 1)) * 100}%` }}
                      >
                        <span>{snapGuide.label}</span>
                      </div>
                    ) : null}
                    <div
                      className="timeline-playhead"
                      style={{ left: `${(Math.min(playheadMs, videoDurationMs) / Math.max(videoDurationMs, 1)) * 100}%` }}
                    />
                    {track.regions.map((region, regionIndex) => {
                      const left = `${(region.start_ms / Math.max(videoDurationMs, 1)) * 100}%`;
                      const width = `${((region.end_ms - region.start_ms) / Math.max(videoDurationMs, 1)) * 100}%`;
                      const isSelected =
                        selected?.trackIndex === trackIndex && selected?.regionId === region.id;

                      return (
                        <div
                          key={region.id}
                          className={`timeline-chip ${isSelected ? "selected" : ""}`}
                          style={{ left, width }}
                          onClick={() => setSelected({ trackIndex, regionId: region.id })}
                          onPointerDown={(event) => beginTimelineDrag(event, trackIndex, regionIndex, "move")}
                          role="button"
                          tabIndex={0}
                        >
                          <div
                            className="timeline-handle start"
                            onPointerDown={(event) => beginTimelineDrag(event, trackIndex, regionIndex, "resize-start")}
                          />
                          <span>{region.label}</span>
                          <small>{formatMs(region.start_ms)} - {formatMs(region.end_ms)}</small>
                          <div
                            className="timeline-handle end"
                            onPointerDown={(event) => beginTimelineDrag(event, trackIndex, regionIndex, "resize-end")}
                          />
                        </div>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </section>

        <aside className="settings-column card">
          <div className="panel-header stacked">
            <div>
              <p className="panel-kicker">Settings</p>
              <h2>{selectedRegion ? selectedRegion.label : "Select a segment"}</h2>
            </div>
            <div className="toolbar-row compact">
              <button className="ghost-button small" onClick={() => void saveProject()} disabled={!session || busy}>
                Save project
              </button>
              <button className="ghost-button small danger" onClick={() => removeSelectedRegion()} disabled={!selectedRegion || busy}>
                Remove
              </button>
            </div>
          </div>

          {selectedRegion ? (
            <div className="settings-form">
              <label>
                Segment label
                <input
                  value={selectedRegion.label}
                  onChange={(event) => {
                    const value = event.target.value;
                    updateSelectedRegion((region) => ({ ...region, label: value }));
                  }}
                />
              </label>

              <label>
                Start
                <input
                  type="range"
                  min={0}
                  max={Math.max(videoDurationMs, 1)}
                  value={selectedRegion.start_ms}
                  onChange={(event) => {
                    const nextStart = Math.min(Number(event.target.value), selectedRegion.end_ms - MIN_REGION_MS);
                    updateSelectedRegion((region) => ({ ...region, start_ms: nextStart }));
                  }}
                />
                <span>{formatMs(selectedRegion.start_ms)}</span>
              </label>

              <label>
                End
                <input
                  type="range"
                  min={Math.max(selectedRegion.start_ms + MIN_REGION_MS, 0)}
                  max={Math.max(videoDurationMs, selectedRegion.start_ms + MIN_REGION_MS)}
                  value={selectedRegion.end_ms}
                  onChange={(event) => {
                    const nextEnd = Math.max(Number(event.target.value), selectedRegion.start_ms + MIN_REGION_MS);
                    updateSelectedRegion((region) => ({ ...region, end_ms: nextEnd }));
                  }}
                />
                <span>{formatMs(selectedRegion.end_ms)}</span>
              </label>

              {selectedRegion.emphasis !== null && (
                <label>
                  {selectedRegion.focus_rect ? "Magnify amount" : "Speed multiplier"}
                  <input
                    type="number"
                    step="0.1"
                    min="0.1"
                    value={selectedRegion.emphasis ?? 1}
                    onChange={(event) => {
                      const next = Number(event.target.value);
                      updateSelectedRegion((region) => ({ ...region, emphasis: Number.isFinite(next) ? next : region.emphasis }));
                    }}
                  />
                </label>
              )}

              {selectedRegion.focus_rect && (
                <div className="focus-grid">
                  {(["x", "y", "width", "height"] as const).map((key) => (
                    <label key={key}>
                      Focus {key}
                      <input
                        type="number"
                        min="0"
                        max="1"
                        step="0.05"
                        value={selectedRegion.focus_rect?.[key] ?? 0}
                        onChange={(event) => {
                          const next = Number(event.target.value);
                          updateSelectedRegion((region) => ({
                            ...region,
                            focus_rect: {
                              ...(region.focus_rect ?? { x: 0.2, y: 0.2, width: 0.6, height: 0.6 }),
                              [key]: Number.isFinite(next) ? next : 0
                            }
                          }));
                        }}
                      />
                    </label>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <div className="empty-sidebar">
              <p>Select a timeline segment to edit Trim, Speed, Magnify, or Text settings.</p>
            </div>
          )}

          <div className="sidebar-section">
            <h3>Export</h3>
            <div className="toolbar-row compact">
              <button className="ghost-button small" onClick={() => void exportCurrentProject("Mp4")} disabled={!session || busy}>
                Export MP4
              </button>
              <button className="accent-button small" onClick={() => void exportCurrentProject("Gif")} disabled={!session || busy}>
                Export GIF
              </button>
            </div>
            {lastExportPath ? <p className="path-note">Last export: {lastExportPath}</p> : null}
          </div>

          <div className="sidebar-section">
            <h3>Project notes</h3>
            <textarea
              value={notesValue}
              onChange={(event) => {
                const notes = event.target.value
                  .split("\n")
                  .map((line) => line.trim())
                  .filter((line) => line.length > 0);
                updateProject((draft) => ({ ...draft, notes }));
              }}
            />
          </div>

          <div className="sidebar-section">
            <h3>Recent sessions</h3>
            <div className="recent-list">
              {recentSessions.map((entry) => (
                <button key={entry.manifest_path} className="recent-item" onClick={() => void loadSession(entry.manifest_path)}>
                  <strong>{entry.source_name}</strong>
                  <span>{entry.stage}</span>
                </button>
              ))}
            </div>
          </div>
        </aside>
      </main>
    </div>
  );
}

function formatMs(value: number) {
  const totalSeconds = Math.floor(value / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function trackDisplayName(kind: TimelineTrackKind) {
  switch (kind) {
    case "Trim":
      return "Trim";
    case "Speed":
      return "Speed";
    case "Zoom":
      return "Magnify";
    case "Annotation":
      return "Text";
  }
}

export default App;

function resolveSelection(project: ProjectDocument, selection: Selection | null | undefined) {
  if (!selection) {
    return null;
  }

  const selectedTrack = project.timeline_tracks[selection.trackIndex];
  if (selectedTrack?.regions.some((region) => region.id === selection.regionId)) {
    return selection;
  }

  for (let trackIndex = 0; trackIndex < project.timeline_tracks.length; trackIndex += 1) {
    const region = project.timeline_tracks[trackIndex].regions.find((entry) => entry.id === selection.regionId);
    if (region) {
      return { trackIndex, regionId: region.id };
    }
  }

  return null;
}

function normalizeProjectDocument(project: ProjectDocument, durationMs: number) {
  return {
    ...project,
    timeline_tracks: project.timeline_tracks.map((track) => ({
      ...track,
      regions: normalizeTrackRegions(track.regions, durationMs)
    }))
  };
}

function normalizeTrackRegions(regions: TimelineRegion[], durationMs: number) {
  const maxDuration = Math.max(durationMs, MIN_REGION_MS);
  let cursor = 0;

  return [...regions]
    .sort(compareRegions)
    .map((region) => {
      let startMs = snapMs(region.start_ms);
      let endMs = snapMs(region.end_ms);

      startMs = clamp(startMs, 0, Math.max(maxDuration - MIN_REGION_MS, 0));
      endMs = clamp(endMs, startMs + MIN_REGION_MS, maxDuration);

      if (startMs < cursor) {
        startMs = cursor;
        endMs = Math.max(endMs, startMs + MIN_REGION_MS);
      }

      if (endMs > maxDuration) {
        endMs = maxDuration;
        startMs = Math.max(0, endMs - MIN_REGION_MS);
      }

      cursor = endMs;

      return {
        ...region,
        start_ms: startMs,
        end_ms: endMs,
        focus_rect: region.focus_rect ? normalizeRect(region.focus_rect) : region.focus_rect
      };
    });
}

function normalizeRect(rect: NormalizedRect) {
  const x = clamp(rect.x, 0, 0.92);
  const y = clamp(rect.y, 0, 0.92);
  const width = clamp(rect.width, 0.08, 1 - x);
  const height = clamp(rect.height, 0.08, 1 - y);

  return { x, y, width, height };
}

function compareRegions(left: TimelineRegion, right: TimelineRegion) {
  if (left.start_ms !== right.start_ms) {
    return left.start_ms - right.start_ms;
  }

  if (left.end_ms !== right.end_ms) {
    return left.end_ms - right.end_ms;
  }

  return left.id.localeCompare(right.id);
}

function collectSnapCandidates(
  tracks: TimelineTrack[],
  activeRegionId: string,
  playheadMs: number,
  durationMs: number
) {
  const candidates: SnapGuide[] = [{ ms: snapMs(playheadMs), label: "Playhead" }];

  for (const track of tracks) {
    for (const region of track.regions) {
      if (region.id === activeRegionId) {
        continue;
      }

      candidates.push({ ms: region.start_ms, label: `${track.label} start` });
      candidates.push({ ms: region.end_ms, label: `${track.label} end` });
    }
  }

  candidates.push({ ms: 0, label: "Start" });
  candidates.push({ ms: durationMs, label: "End" });

  return candidates;
}

function snapValue(value: number, candidates: SnapGuide[], edgeLabel: string) {
  let bestGuide: SnapGuide | null = null;
  let bestDistance = SNAP_MS;

  for (const candidate of candidates) {
    const distance = Math.abs(candidate.ms - value);
    if (distance <= bestDistance) {
      bestDistance = distance;
      bestGuide = candidate;
    }
  }

  if (bestGuide) {
    return {
      value: bestGuide.ms,
      guide: { ms: bestGuide.ms, label: `${bestGuide.label} ${edgeLabel}` }
    };
  }

  return { value: snapMs(value), guide: null };
}

function snapMoveBounds(
  startMs: number,
  lengthMs: number,
  candidates: SnapGuide[],
  minStart: number,
  maxEnd: number
) {
  const snappedStart = snapValue(startMs, candidates, "start");
  const snappedEnd = snapValue(startMs + lengthMs, candidates, "end");

  let nextStart = snappedStart.value;
  let nextGuide = snappedStart.guide;

  if (snappedEnd.guide) {
    const endAnchoredStart = snappedEnd.value - lengthMs;
    const endDistance = Math.abs((startMs + lengthMs) - snappedEnd.value);
    const startDistance = Math.abs(startMs - snappedStart.value);

    if (endAnchoredStart >= minStart && endAnchoredStart + lengthMs <= maxEnd && endDistance <= startDistance) {
      nextStart = endAnchoredStart;
      nextGuide = snappedEnd.guide;
    }
  }

  nextStart = clamp(nextStart, minStart, Math.max(maxEnd - lengthMs, minStart));

  return {
    startMs: nextStart,
    endMs: nextStart + lengthMs,
    guide: nextGuide
  };
}

function snapMs(value: number) {
  return Math.round(value / SNAP_MS) * SNAP_MS;
}

function activeRegionForKind(project: ProjectDocument, kind: TimelineTrackKind, sampleMs: number) {
  const track = project.timeline_tracks.find((entry) => entry.kind === kind);

  return track?.regions.find((region) => sampleMs >= region.start_ms && sampleMs < region.end_ms) ?? null;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}