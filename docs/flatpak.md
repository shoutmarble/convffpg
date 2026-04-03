# Flatpak Scaffold

This repository includes a starter Flatpak manifest at `flatpak/io.github.terry.convffpg.yml`.

Why Flatpak is a good fit here:

- it is better suited than a single self-contained binary for GStreamer-based desktop capture
- it works well with Wayland portals and PipeWire on Linux desktops
- it gives you a cleaner place to ship media runtime dependencies than embedding `-dev` packages in the Rust executable

What Flatpak does not do here:

- it does not replace host-installed GStreamer or PipeWire development headers for native `cargo build`
- it does not replace the host portal service or host PipeWire daemon required by Wayland capture

Current manifest notes:

- runtime: `org.freedesktop.Platform//24.08`
- sdk: `org.freedesktop.Sdk//24.08`
- command: `convffpg`
- the manifest now builds with `cargo --frozen --offline build --release`
- the repository includes vendored cargo sources under `vendor/` plus `.cargo/config.toml`
- the manifest uses `org.freedesktop.Sdk.Extension.rust-stable//24.08` so cargo is available inside the Flatpak build sandbox
- the manifest installs a desktop file, AppStream metadata, and a scalable SVG icon from `flatpak/`
- screenshot assets are included at `flatpak/io.github.terry.convffpg-screenshot.svg` and `flatpak/io.github.terry.convffpg-screenshot.png`
- the final hosted AppStream metadata can be rendered from `flatpak/io.github.terry.convffpg.metainfo.xml.in`
- a helper validation script is included at `flatpak/validate-packaging.sh`
- a helper build script is included at `flatpak/build-flatpak.sh`
- a helper runtime installer is included at `scripts/install-flatpak-runtimes.sh`
- a GitHub Actions workflow validates the metadata and Flatpak manifest on Ubuntu when CI is available

Before this becomes production-ready, you will likely want to:

- add any runtime extensions or modules needed for GStreamer plugin coverage
- validate the desktop metadata against `appstreamcli` and your target store requirements
- replace the screenshot mockup assets with a real application screenshot and wire the final hosted image into the AppStream delivery flow
- verify the exact portal and filesystem permissions you want to ship

Typical local workflow:

```bash
flatpak-builder --user --install --force-clean build-flatpak flatpak/io.github.terry.convffpg.yml
flatpak run io.github.terry.convffpg
```

Packaging validation workflow when the tools are installed:

```bash
./flatpak/validate-packaging.sh
```

Final AppStream rendering workflow when you have real public URLs:

```bash
APP_HOMEPAGE_URL="https://your-project-homepage" \
APP_SCREENSHOT_URL="https://your-public-screenshot-url" \
./scripts/render-metainfo.sh
```

Flatpak build workflow when `flatpak-builder` is installed:

```bash
./flatpak/build-flatpak.sh
```

If you change Rust dependencies, refresh the vendored tree with:

```bash
cargo vendor vendor > .cargo/config.toml
```

If you keep FFmpeg bundled inside the application itself, Flatpak is mainly solving the desktop media runtime side: GStreamer, plugins, portals, and sandboxed integration.

Current environment note:

- `flatpak` is installed on this machine
- `flatpak-builder` is not installed here yet
- `appstreamcli` is installed on this machine
- `desktop-file-validate` is installed on this machine
- the Wayland capture prerequisites also still require privileged system package installation

Repository-side automation note:

- `.github/workflows/packaging.yml` installs the validation tools and runs both helper scripts in CI
- `scripts/install-flatpak-runtimes.sh` installs the Flatpak SDK/runtime side that the packaging workflow needs
- `scripts/install-flatpak-runtimes.sh` also installs the Rust SDK extension needed by the Flatpak build
- `scripts/install-ubuntu-prereqs.sh` installs the local Ubuntu packages needed for Wayland capture and packaging validation
- `scripts/check-wayland-portal.sh` prints the current portal, packaging-tool, and pkg-config status on a Linux host
- `scripts/render-metainfo.sh` turns the AppStream template into a final metadata file once you have real hosted homepage and screenshot URLs