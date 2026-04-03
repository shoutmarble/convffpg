#!/usr/bin/env bash

set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  if command -v sudo >/dev/null 2>&1; then
    exec sudo "$0" "$@"
  fi

  echo "Run this script as root or install sudo first." >&2
  exit 1
fi

if [[ -f /etc/os-release ]]; then
  . /etc/os-release
else
  echo "Unable to detect Linux distribution." >&2
  exit 1
fi

if [[ "${ID:-}" =~ ^(ubuntu|debian|linuxmint|pop|neon)$ ]] \
  || [[ " ${ID_LIKE:-} " == *" ubuntu "* ]] \
  || [[ " ${ID_LIKE:-} " == *" debian "* ]]; then
    apt-get update
    apt-get install -y \
      build-essential \
      curl \
      wget \
      file \
      pkg-config \
      libatk1.0-dev \
      libgdk-pixbuf-2.0-dev \
      libgtk-3-dev \
      libwebkit2gtk-4.1-dev \
      libayatana-appindicator3-dev \
      librsvg2-dev
elif [[ "${ID:-}" == "fedora" ]]; then
    dnf install -y \
      @development-tools \
      curl \
      wget \
      file \
      pkgconf-pkg-config \
      gtk3-devel \
      webkit2gtk4.1-devel \
      libappindicator-gtk3-devel \
      librsvg2-devel
elif [[ "${ID:-}" =~ ^(arch|endeavouros)$ ]]; then
    pacman -Syu --needed --noconfirm \
      base-devel \
      curl \
      wget \
      file \
      pkgconf \
      gtk3 \
      webkit2gtk-4.1 \
      libappindicator-gtk3 \
      librsvg
else
    echo "Unsupported distribution: ${ID:-unknown}" >&2
    echo "Install the Tauri Linux prerequisites manually:" >&2
    echo "  build-essential, pkg-config, atk dev files, gdk-pixbuf dev files, gtk3 dev files, webkit2gtk 4.1 dev files, appindicator dev files, librsvg dev files" >&2
    exit 1
fi

echo "Linux Tauri prerequisites installed."