#!/usr/bin/env bash
set -euo pipefail

if ! command -v flatpak >/dev/null 2>&1; then
  echo "flatpak is missing. Install Flatpak first, then rerun this script."
  exit 1
fi

flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y flathub \
  org.freedesktop.Platform//24.08 \
  org.freedesktop.Sdk//24.08 \
  org.freedesktop.Sdk.Extension.rust-stable//24.08
