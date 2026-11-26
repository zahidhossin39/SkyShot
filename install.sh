#!/bin/sh
# Install (or update) Sky Shot on macOS or Linux:
#
#   curl -fsSL https://raw.githubusercontent.com/zahidhossin39/sky-shot/main/install.sh | sh
#
# macOS: the universal app into /Applications.
# Linux: .deb on Debian/Ubuntu/Mint/Pop, .rpm on Fedora/RHEL/openSUSE, otherwise the
# self-updating AppImage in ~/.local/bin (Arch, immutable distros, everything else).
# Set SKYSHOT_FORMAT=appimage|deb|rpm to choose yourself. On Linux the package and command are
# `sky-shot`. Sky Shot used to be called Shutter; an old copy is replaced.
set -eu

REPO="zahidhossin39/sky-shot"
say() { printf '%s\n' "$*"; }
fail() { say "Sky Shot install: $*" >&2; exit 1; }
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
  fetch "$(pick 'universal\.app\.tar\.gz$')" "$tmp/app.tar.gz"
  tar -xzf "$tmp/app.tar.gz" -C "$tmp"
  [ -d "$tmp/Sky Shot.app" ] || fail "unexpected download contents"
  app="/Applications/Sky Shot.app"
  osascript -e 'quit app "Sky Shot"' -e 'quit app "Shutter"' >/dev/null 2>&1 || true
  if [ -w /Applications ]; then rm -rf "$app" /Applications/Shutter.app && mv "$tmp/Sky Shot.app" /Applications/
  else as_root rm -rf "$app" /Applications/Shutter.app && as_root mv "$tmp/Sky Shot.app" /Applications/; fi
  # Downloaded by curl, so there's no quarantine flag; clear it anyway in case.
  xattr -dr com.apple.quarantine "$app" 2>/dev/null || true
  say "Installed $app"
  say "First capture: macOS asks for Screen Recording permission (System Settings >"
  say "Privacy & Security). Turn Sky Shot on there, then reopen it."
  open "$app"
}

install_appimage() {
  bin="$HOME/.local/bin"
  mkdir -p "$bin" "$HOME/.local/share/applications"
  fetch "$(pick '\.AppImage$')" "$tmp/SkyShot.AppImage"
  chmod +x "$tmp/SkyShot.AppImage"
  mv -f "$tmp/SkyShot.AppImage" "$bin/SkyShot.AppImage"
  # The old name (Shutter) and its menu entry.
  rm -f "$bin/Shutter.AppImage" "$bin/shutter-app" "$HOME/.local/share/applications/shutter.desktop"
  exec_line="$bin/SkyShot.AppImage"
  # AppImages mount with FUSE 2, which newer Ubuntu doesn't ship; run unpacked instead.
  if ! ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2'; then
    exec_line="env APPIMAGE_EXTRACT_AND_RUN=1 $exec_line"
    say "Note: FUSE 2 not found, so Sky Shot runs unpacked (a little slower to start)."
  fi
  cat >"$HOME/.local/share/applications/sky-shot.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Sky Shot
Comment=Screenshots with blur, OCR and annotations
Exec=$exec_line
Categories=Utility;Graphics;
Terminal=false
EOF
  ln -sf "$bin/SkyShot.AppImage" "$bin/sky-shot"
  say "Installed $bin/SkyShot.AppImage (updates itself from Settings)"
}

install_linux() {
  [ "$(uname -m)" = x86_64 ] || fail "only x86_64 builds so far (this is $(uname -m))"
  id_like=""
  if [ -r /etc/os-release ]; then id_like=$(. /etc/os-release && echo "${ID:-} ${ID_LIKE:-}"); fi
  format=${SKYSHOT_FORMAT:-}
  if [ -z "$format" ]; then
    if have apt-get && echo "$id_like" | grep -qiE 'debian|ubuntu'; then format=deb
    elif { have dnf || have zypper; } && echo "$id_like" | grep -qiE 'fedora|rhel|centos|suse'; then format=rpm
    else format=appimage; fi
  fi
  case "$format" in
    deb)
      fetch "$(pick '_amd64\.deb$')" "$tmp/sky-shot.deb"
      as_root apt-get install -y "$tmp/sky-shot.deb" ;;
    rpm)
      fetch "$(pick '\.x86_64\.rpm$')" "$tmp/sky-shot.rpm"
      if have dnf; then as_root dnf install -y "$tmp/sky-shot.rpm"
      else as_root zypper --non-interactive install --allow-unsigned-rpm "$tmp/sky-shot.rpm"; fi ;;
    appimage) install_appimage ;;
    *) fail "unknown SKYSHOT_FORMAT '$format' (use appimage, deb or rpm)" ;;
  esac
  say "Done. Open Sky Shot from your app menu."
  if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    say "Wayland: apps can't grab global hotkeys there. Add a shortcut in your desktop's"
    say "keyboard settings that runs:  sky-shot --capture"
  fi
}

case "$(uname -s)" in
  Darwin) install_mac ;;
  Linux) install_linux ;;
  *) fail "this script is for macOS and Linux; on Windows use the installer from the releases page" ;;
esac
