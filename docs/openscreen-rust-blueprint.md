# OpenScreen Rust Blueprint

This document maps the current Electron app at `siddharthvaddem/openscreen` to a realistic Rust implementation plan.

## What the upstream app actually is

OpenScreen is not a simple screen recorder.

It combines:

- a floating HUD / launch window
- a screen-or-window source picker
- screen, mic, system-audio, and optional webcam recording
- session storage for recorded media
- cursor telemetry capture for auto zoom suggestions
- a timeline editor with zoom, trim, speed, and annotation tracks
- a preview renderer with crop, padding, blur, shadow, and webcam layouts
- GIF and MP4 export with progress reporting
- project save/load and tray/menu integration

From the upstream repository structure, the main subsystems are:

- Electron shell and IPC bridge: `electron/main.ts`, `electron/preload.ts`, `electron/ipc/handlers.ts`
- Launch and source selection UI: `src/components/launch/*`
- Recording flow: `src/hooks/useScreenRecorder.ts`
- Editor and persistence: `src/components/video-editor/*`
- Rendering and export: `src/lib/exporter/*`

## Recommended Rust architecture

If the goal is "Rust version of OpenScreen" and not "bit-for-bit port of the web stack", the cleanest architecture is:

### 1. Native shell and UI

Use `iced 0.14` for the desktop UI.

Why:

- already in use in this repository
- native Rust event loop and windowing
- good fit for multi-window desktop tooling
- no Electron runtime dependency

Tradeoff:

- building a rich timeline editor and canvas-heavy preview in pure `iced` is significantly more work than the original React + PixiJS stack

### 2. Capture backend

Use `gstreamer 0.25.1` for capture on Linux, especially Wayland/PipeWire.

Why:

- Electron's `desktopCapturer` advantage disappears in a Rust rewrite
- Linux screen/system-audio capture is much more practical through PipeWire/GStreamer than through handwritten FFmpeg bindings
- webcam, mic, muxing, and pipeline control are already first-class concerns in GStreamer

Avoid making `ffmpeg-next` the primary capture layer.

Reason:

- `ffmpeg-next 8.1.0` is maintenance-oriented
- it is better suited as a lower-level codec wrapper than as the whole desktop capture story
- Linux capture on Wayland is the hard part, and GStreamer is the better tool there

### 3. Export pipeline

Use bundled FFmpeg binaries for export jobs.

Why:

- deterministic export behavior across machines
- easier to support GIF and MP4 output profiles
- easier to iterate on filters, crop, scale, trim, speed changes, and muxing

This matches the direction already started in this repository.

### 4. Data model and persistence

Use a versioned Rust project schema serialized as JSON.

The upstream project file effectively needs these model groups:

- project media
  - screen video path
  - optional webcam video path
- cursor telemetry
  - `time_ms`
  - normalized `cx`
  - normalized `cy`
- zoom regions
  - id
  - start/end ms
  - depth
  - focus point
- trim regions
  - id
  - start/end ms
- speed regions
  - id
  - start/end ms
  - playback speed
- annotation regions
  - type: text, image, figure
  - content / style / size / position / z-index
- composite settings
  - wallpaper/background
  - crop region
  - border radius
  - padding
  - blur and shadow
  - aspect ratio
  - webcam layout preset
  - webcam position
- export settings
  - format: mp4 or gif
  - export quality
  - gif frame rate
  - gif loop
  - gif size preset

## What should be built first

Do not try to recreate full OpenScreen parity in one pass.

That would force you to solve, all at once:

- Wayland capture
- desktop source enumeration
- webcam composition
- timeline editing
- annotation rendering
- frame-accurate export
- multi-window UX
- tray/menu integration
- project persistence and migrations

Build it in phases.

## Phase plan

### Phase 1: Recording MVP

Goal: produce an actual usable recorder/editor baseline.

Scope:

- source picker for screens/windows
- start/stop recording
- optional microphone toggle
- session save to app data directory
- open recorded session in editor
- simple export to GIF and MP4

Rust stack:

- `iced` for windows
- `gstreamer` for capture
- bundled FFmpeg CLI for export
- `serde` + `serde_json` for session manifests

### Phase 2: Editor MVP

Goal: reach the "good enough demo tool" threshold.

Scope:

- video preview
- trim regions
- speed regions
- crop region
- aspect ratio presets
- basic zoom regions
- output progress and save/reveal actions

Rust implementation note:

- if preview rendering becomes too complex in `iced` alone, introduce a custom `wgpu` render surface for the editor canvas instead of forcing every effect through basic widgets

### Phase 3: OpenScreen-like polish

Goal: close the gap with the Electron app.

Scope:

- cursor telemetry capture and auto zoom suggestions
- webcam compositing presets
- annotations: text, arrow, image
- tray integration
- keyboard shortcuts editor
- project save/load
- resilient export cancellation and resume flows

### Phase 4: Packaging

Goal: ship stable Linux builds first.

Scope:

- bundle FFmpeg for export
- document GStreamer/PipeWire runtime requirements clearly
- package AppImage or distro-specific builds
- later add Windows and macOS support

## Crate recommendations

Current relevant versions observed:

- `iced = 0.14.0`
- `gstreamer = 0.25.1`
- `tauri = 2.10.3`
- `ffmpeg-next = 8.1.0`

Recommended usage:

- `iced`: yes, for native Rust UI
- `gstreamer`: yes, for recording/capture pipelines
- `ffmpeg-next`: not as the primary app architecture
- `tauri`: only if you decide that shipping a Rust backend with a web frontend is acceptable

## Two viable rewrite strategies

### Strategy A: All-Rust desktop app

Stack:

- `iced`
- `gstreamer`
- bundled FFmpeg

Pros:

- true Rust-native application
- no browser app embedded in the shell
- aligns with the current repository direction

Cons:

- longest path to timeline/editor parity
- more rendering and interaction code must be built from scratch

### Strategy B: Rust backend, web editor frontend

Stack:

- `tauri`
- Rust commands/backend
- web frontend for the editor surface

Pros:

- fastest route to OpenScreen-like editor UX
- easier canvas/timeline/annotation work
- still replaces Electron with Rust-native shell/backend

Cons:

- not an all-Rust UI
- you still maintain a frontend stack

## Recommendation

If your real goal is feature parity with OpenScreen, do not start by rewriting everything into a pure `iced` editor.

The pragmatic path is:

1. keep this repository as a Rust-native recorder/exporter foundation
2. add `gstreamer` recording and source selection first
3. store sessions and telemetry in a Rust project model
4. only then build the richer editor surface

If you want a strictly all-Rust application, accept that the timeline/editor is the expensive part and plan the work in phases.

## Suggested next implementation target in this repo

The next meaningful milestone for this repository is:

- replace the current single-purpose converter mindset with a capture session model
- add a source selection window
- record screen + mic into a session manifest
- open the captured file in a basic editor window
- keep the current FFmpeg export path for GIF output

That gives you a credible "Rust OpenScreen MVP" instead of a large but non-functional rewrite.