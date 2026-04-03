# Linux Capture Dependency Model

This project now has two different dependency layers on Linux, and they should not be mixed up.

## 1. Native host build dependencies

If you want to build and run the recorder natively with `cargo build` and later add a real GStreamer or PipeWire backend, the compiler and linker need Linux development headers and `pkg-config` metadata from the host system.

On Ubuntu or Debian, that means packages like:

```bash
sudo apt-get update
sudo apt-get install -y libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libpipewire-0.3-dev
```

Flatpak does not replace these for a native build.

## 2. Flatpak build and runtime dependencies

If you package the app as a Flatpak, the sandboxed build uses the Flatpak SDK and runtime instead of your host compiler environment.

That means you can install the SDK and runtime with Flatpak itself, for example:

```bash
flatpak install -y flathub org.freedesktop.Platform//24.08 org.freedesktop.Sdk//24.08
```

This helps the Flatpak packaging path, but it does not make the host `cargo build` suddenly able to compile against GStreamer headers.

## 3. Wayland capture still depends on host services

Even when the app is shipped as a Flatpak, Wayland capture still depends on host-side services:

- `xdg-desktop-portal`
- a compatible portal backend such as GTK or KDE
- host PipeWire service availability

Flatpak can bundle app-side runtime pieces. It does not own the compositor, portal service, or the host PipeWire daemon.

## 4. Runtime tools and plugins still matter

Even after the development headers are installed, a GStreamer-based Wayland recorder still needs runtime packages such as:

```bash
sudo apt-get update
sudo apt-get install -y gstreamer1.0-tools gstreamer1.0-pipewire xdg-desktop-portal xdg-desktop-portal-gtk
```

In practice, the quickest checks are:

```bash
command -v gst-launch-1.0
command -v gst-inspect-1.0
gst-inspect-1.0 pipewiresrc
```

## Practical rule

- Use `apt`, `dnf`, `pacman`, or the host distro package manager for native development headers.
- Use Flatpak to install the SDK/runtime and to package the app for distribution.
- Expect Wayland capture to require both a good Flatpak setup and a healthy host portal/PipeWire setup.

## Current project status

- The repository now has a first-pass Wayland screen recording path that opens the desktop portal picker, obtains a PipeWire stream through `ashpd`, and records it with `gst-launch-1.0`.
- The current Wayland path supports display and window capture.
- Region capture and microphone capture are still pending.