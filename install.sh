#!/bin/sh
# Install (or update) Shutter on macOS or Linux:
#
#   curl -fsSL https://raw.githubusercontent.com/zahidhossin39/shutter/main/install.sh | sh
#
# macOS: the universal app into /Applications.
# Linux: .deb on Debian/Ubuntu/Mint/Pop, .rpm on Fedora/RHEL/openSUSE, otherwise the
# self-updating AppImage in ~/.local/bin (Arch, immutable distros, everything else).
# Set SHUTTER_FORMAT=appimage|deb|rpm to choose yourself. On Linux the package and command are
# `shutter-app`: Debian/Ubuntu already have an unrelated `shutter` package.
set -eu

REPO="zahidhossin39/shutter"
say() { printf '%s\n' "$*"; }
fail() { say "Shutter install: $*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
as_root() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }

have curl || fail "curl is needed"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

# Download links of the newest release, from the updater's latest.json: a plain download
# (the GitHub API allows only 60 requests an hour per network). No jq needed.
assets=$(curl -fsSL "https://github.com/$REPO/releases/latest/download/latest.json" |
  grep -o '"url": *"[^"]*"' | sed 's/.*"\(https[^"]*\)"$/\1/' | sort -u) ||
  fail "couldn't reach GitHub"
pick() { printf '%s\n' "$assets" | grep -E "$1" | head -n 1; }
fetch() { # url file
  [ -n "$1" ] || fail "this release has no build for your system yet"
  say "Downloading $(basename "$1")"
  curl -fL --progress-bar -o "$2" "$1"
}

install_mac() {
  # The universal .app (both chips), the same archive the in-app updater uses.
  fetch "$(pick 'universal\.app\.tar\.gz$')" "$tmp/Shutter.app.tar.gz"
  tar -xzf "$tmp/Shutter.app.tar.gz" -C "$tmp"
  [ -d "$tmp/Shutter.app" ] || fail "unexpected download contents"
  app="/Applications/Shutter.app"
  osascript -e 'quit app "Shutter"' >/dev/null 2>&1 || true
  if [ -w /Applications ]; then rm -rf "$app" && mv "$tmp/Shutter.app" /Applications/
  else as_root rm -rf "$app" && as_root mv "$tmp/Shutter.app" /Applications/; fi
  # Downloaded by curl, so there's no quarantine flag; clear it anyway in case.
  xattr -dr com.apple.quarantine "$app" 2>/dev/null || true
  say "Installed $app"
  say "First capture: macOS asks for Screen Recording permission (System Settings >"
  say "Privacy & Security). Turn Shutter on there, then reopen it."
  open "$app"
}

install_appimage() {
  bin="$HOME/.local/bin"
  mkdir -p "$bin" "$HOME/.local/share/applications"
  fetch "$(pick '\.AppImage$')" "$tmp/Shutter.AppImage"
  chmod +x "$tmp/Shutter.AppImage"
  mv -f "$tmp/Shutter.AppImage" "$bin/Shutter.AppImage"
  exec_line="$bin/Shutter.AppImage"
  # AppImages mount with FUSE 2, which newer Ubuntu doesn't ship; run unpacked instead.
  if ! ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2'; then
    exec_line="env APPIMAGE_EXTRACT_AND_RUN=1 $exec_line"
    say "Note: FUSE 2 not found, so Shutter runs unpacked (a little slower to start)."
  fi
  cat >"$HOME/.local/share/applications/shutter.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Shutter
Comment=Screenshots with blur, OCR and annotations
Exec=$exec_line
Categories=Utility;Graphics;
Terminal=false
EOF
  ln -sf "$bin/Shutter.AppImage" "$bin/shutter-app"
  say "Installed $bin/Shutter.AppImage (updates itself from Settings)"
}

install_linux() {
  [ "$(uname -m)" = x86_64 ] || fail "only x86_64 builds so far (this is $(uname -m))"
  id_like=""
  if [ -r /etc/os-release ]; then id_like=$(. /etc/os-release && echo "${ID:-} ${ID_LIKE:-}"); fi
  format=${SHUTTER_FORMAT:-}
  if [ -z "$format" ]; then
    if have apt-get && echo "$id_like" | grep -qiE 'debian|ubuntu'; then format=deb
    elif { have dnf || have zypper; } && echo "$id_like" | grep -qiE 'fedora|rhel|centos|suse'; then format=rpm
    else format=appimage; fi
  fi
  case "$format" in
    deb)
      fetch "$(pick '_amd64\.deb$')" "$tmp/shutter.deb"
      as_root apt-get install -y "$tmp/shutter.deb" ;;
    rpm)
      fetch "$(pick '\.x86_64\.rpm$')" "$tmp/shutter.rpm"
      if have dnf; then as_root dnf install -y "$tmp/shutter.rpm"
      else as_root zypper --non-interactive install --allow-unsigned-rpm "$tmp/shutter.rpm"; fi ;;
    appimage) install_appimage ;;
    *) fail "unknown SHUTTER_FORMAT '$format' (use appimage, deb or rpm)" ;;
  esac
  say "Done. Open Shutter from your app menu."
  if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    say "Wayland: apps can't grab global hotkeys there. Add a shortcut in your desktop's"
    say "keyboard settings that runs:  shutter-app --capture"
  fi
}

case "$(uname -s)" in
  Darwin) install_mac ;;
  Linux) install_linux ;;
  *) fail "this script is for macOS and Linux; on Windows use the installer from the releases page" ;;
esac
