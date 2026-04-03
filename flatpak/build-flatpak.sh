#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root_dir/flatpak/io.github.terry.convffpg.yml"
build_dir="$root_dir/build-flatpak"

if ! command -v flatpak-builder >/dev/null 2>&1; then
  echo "flatpak-builder is missing. Install it first, then rerun this script."
  exit 1
fi

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is required to build frontend/dist before the Flatpak build."
  exit 1
fi

echo "Building frontend bundle for the Flatpak payload..."
(cd "$root_dir" && npm run build)

flatpak-builder --user --install --force-clean "$build_dir" "$manifest"
