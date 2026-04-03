#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y \
  appstream \
  desktop-file-utils \
  flatpak \
  flatpak-builder \
  gstreamer1.0-pipewire \
  gstreamer1.0-tools \
  libgstreamer-plugins-base1.0-dev \
  libgstreamer1.0-dev \
  libpipewire-0.3-dev \
  x11-xserver-utils \
  xdg-desktop-portal \
  xdg-desktop-portal-gtk
