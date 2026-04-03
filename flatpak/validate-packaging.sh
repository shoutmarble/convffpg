#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
metainfo="$root_dir/flatpak/io.github.terry.convffpg.metainfo.xml"
desktop_file="$root_dir/flatpak/io.github.terry.convffpg.desktop"
manifest="$root_dir/flatpak/io.github.terry.convffpg.yml"
screenshot="$root_dir/flatpak/io.github.terry.convffpg-screenshot.svg"
png_screenshot="$root_dir/flatpak/io.github.terry.convffpg-screenshot.png"
metainfo_template="$root_dir/flatpak/io.github.terry.convffpg.metainfo.xml.in"

echo "Checking packaging assets..."
test -f "$metainfo"
test -f "$desktop_file"
test -f "$manifest"
test -f "$screenshot"
test -f "$png_screenshot"
test -f "$metainfo_template"

if ! command -v desktop-file-validate >/dev/null 2>&1; then
  echo "desktop-file-validate is missing. Install desktop-file-utils to validate the desktop entry."
else
  echo "Validating desktop entry..."
  desktop-file-validate "$desktop_file"
fi

if ! command -v appstreamcli >/dev/null 2>&1; then
  echo "appstreamcli is missing. Install appstream to validate AppStream metadata."
else
  echo "Validating AppStream metadata..."
  appstream_output_file="$(mktemp)"
  if appstreamcli validate --no-net "$metainfo" >"$appstream_output_file" 2>&1; then
    cat "$appstream_output_file"
  else
    cat "$appstream_output_file"

    if ! grep -q '^E:' "$appstream_output_file" \
      && grep -q 'url-homepage-missing' "$appstream_output_file" \
      && [[ "$(grep -c '^W:' "$appstream_output_file")" -eq 1 ]]; then
      echo "Allowing the baseline homepage warning because the final public URL has not been assigned yet."
    else
      rm -f "$appstream_output_file"
      exit 1
    fi
  fi
  rm -f "$appstream_output_file"
  if grep -q '<url type="homepage">https://github.com/shoutmarble/convffpg</url>' "$metainfo" \
    && grep -q 'https://raw.githubusercontent.com/shoutmarble/convffpg/main/flatpak/io.github.terry.convffpg-screenshot.png' "$metainfo"; then
    echo "Hosted homepage and screenshot URLs are configured in the checked-in metainfo."
  else
    echo "Run ./scripts/render-metainfo.sh if you want to inject alternate hosted URLs into the AppStream metadata."
  fi
fi

if ! command -v flatpak-builder >/dev/null 2>&1; then
  echo "flatpak-builder is missing. Install flatpak-builder to test the Flatpak manifest."
else
  echo "Flatpak manifest available at: $manifest"
  echo "Run: flatpak-builder --user --install --force-clean build-flatpak $manifest"
fi

echo "Packaging asset check complete."

