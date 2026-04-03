#!/usr/bin/env bash
set -euo pipefail

echo "Session type: ${XDG_SESSION_TYPE:-unknown}"
echo "WAYLAND_DISPLAY: ${WAYLAND_DISPLAY:-unset}"

if command -v busctl >/dev/null 2>&1; then
  if busctl --user status org.freedesktop.portal.Desktop >/dev/null 2>&1; then
    echo "Portal service: reachable via busctl"
  else
    echo "Portal service: not reachable via busctl"
  fi
elif command -v dbus-send >/dev/null 2>&1; then
  if dbus-send --session --dest=org.freedesktop.portal.Desktop --type=method_call --print-reply /org/freedesktop/portal/desktop org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; then
    echo "Portal service: reachable via dbus-send"
  else
    echo "Portal service: not reachable via dbus-send"
  fi
else
  echo "Portal service: no D-Bus inspection tool available"
fi

if [[ -d /usr/share/xdg-desktop-portal/portals ]]; then
  echo "Installed portal backend files:"
  find /usr/share/xdg-desktop-portal/portals -maxdepth 1 -type f -name '*.portal' -printf '  %f\n' | sort
else
  echo "Installed portal backend files: none detected"
fi

for command_name in flatpak flatpak-builder appstreamcli desktop-file-validate pkg-config xrandr xdg-desktop-portal gst-launch-1.0 gst-inspect-1.0; do
  if command -v "$command_name" >/dev/null 2>&1; then
    echo "$command_name: available"
  else
    echo "$command_name: missing"
  fi
done

for package_name in gstreamer-1.0 gstreamer-plugins-base-1.0 libpipewire-0.3; do
  if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists "$package_name"; then
    echo "$package_name: pkg-config ok"
  else
    echo "$package_name: pkg-config missing"
  fi
done

if command -v gst-inspect-1.0 >/dev/null 2>&1; then
  if gst-inspect-1.0 pipewiresrc >/dev/null 2>&1; then
    echo "pipewiresrc: available"
  else
    echo "pipewiresrc: missing"
  fi
else
  echo "pipewiresrc: unknown because gst-inspect-1.0 is missing"
fi
