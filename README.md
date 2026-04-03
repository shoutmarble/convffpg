# convffpg

`convffpg` is now structured as a Tauri desktop app with a shared Rust media backend.

The current focus is an imported-MP4-first editor flow that is much closer to OpenScreen's usability model than the earlier native `iced` shell. The Rust side still owns bundled FFmpeg extraction, session persistence, and timeline export. The Tauri/React side now owns the editor shell, preview layout, timeline rows, and settings rail.

## Current architecture

- `src/` contains the reusable Rust core.
- `src-tauri/` contains the Tauri desktop shell and command bridge.
- `frontend/` contains the React/Vite editor UI.
- The bundled FFmpeg archive still lives at `assets/ffmpeg/ffmpeg-linux-x86_64.tar.xz` and is unpacked on first run into `~/.local/share/convffpg/bundled-ffmpeg/<bundle-version>/`.
- Imported media sessions are stored under `~/.local/share/convffpg/recording-sessions/` with `session.json`, `project.json`, copied source media, and exported outputs.

## Current behavior

- Import an MP4 and copy it into a persisted session.
- Reopen recent sessions from disk.
- Preview the imported video inside the Tauri shell.
- Add and edit `Trim`, `Speed`, `Magnify`, and `Text` timeline regions.
- Save the edited `project.json` back to disk.
- Export the edited result as MP4 or GIF through the bundled FFmpeg pipeline already present in the Rust backend.

## Build and run

Install the JavaScript dependencies:

```bash
npm install
```

Run the app in development:

```bash
npm run tauri:dev
```

Run the packaged Flatpak app:

```bash
flatpak run io.github.terry.convffpg
```

Rebuild and reinstall the Flatpak package, then run it:

```bash
./flatpak/build-flatpak.sh
flatpak run io.github.terry.convffpg
```

## Checks

Compile-check the desktop shell without launching it:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Compile-check the shared Rust backend without the desktop shell:

```bash
cargo check --lib
```

Build the frontend bundle without launching the desktop shell:

```bash
npm run build
```

## Linux desktop prerequisites

The shared Rust library compiles successfully in the current environment, and the Tauri shell now validates with:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

On Debian, Ubuntu, or KDE neon systems, the current Tauri stack needs at least:

- `libgtk-3-dev`
- `libatk1.0-dev`
- `libgdk-pixbuf-2.0-dev`
- `libwebkit2gtk-4.1-dev`
- `libayatana-appindicator3-dev`
- `librsvg2-dev`

The helper script now detects KDE neon correctly:

```bash
./scripts/install-tauri-linux-prereqs.sh
```

## Flatpak

The Flatpak manifest at [flatpak/io.github.terry.convffpg.yml](/home/terry/Documents/GIT/convffpg/flatpak/io.github.terry.convffpg.yml) packages the Tauri binary instead of the retired root binary.

The helper script builds the frontend bundle on the host first and then runs `flatpak-builder`:

```bash
./flatpak/build-flatpak.sh
```

## Notes

- The earlier `iced` shell has been dropped from Cargo's active targets. The old source file remains in the repo as historical implementation context, but the active path is now Tauri.
- The recorder-oriented Rust modules and Wayland notes are still useful reference material, but the current UX path is imported-video-first rather than live-recording-first.
- A starter Flatpak manifest is still available at [flatpak/io.github.terry.convffpg.yml](/home/terry/Documents/GIT/convffpg/flatpak/io.github.terry.convffpg.yml).
- If you want broader product direction notes for an OpenScreen-style Rust rewrite, see [docs/openscreen-rust-blueprint.md](/home/terry/Documents/GIT/convffpg/docs/openscreen-rust-blueprint.md).
