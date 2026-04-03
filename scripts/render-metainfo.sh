#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
template="$root_dir/flatpak/io.github.terry.convffpg.metainfo.xml.in"
output="$root_dir/flatpak/io.github.terry.convffpg.metainfo.xml"

homepage_url="${APP_HOMEPAGE_URL:-https://github.com/shoutmarble/convffpg}"
screenshot_url="${APP_SCREENSHOT_URL:-https://raw.githubusercontent.com/shoutmarble/convffpg/main/flatpak/io.github.terry.convffpg-screenshot.png}"

sed \
  -e "s|{{HOMEPAGE_URL}}|$homepage_url|g" \
  -e "s|{{SCREENSHOT_URL}}|$screenshot_url|g" \
  "$template" > "$output"

echo "Rendered $output"
