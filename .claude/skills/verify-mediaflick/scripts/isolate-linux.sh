#!/usr/bin/env bash
# Linux isolation wrapper for verify-mediaflick: runs one command against a
# private Xvfb display, with no Wayland socket, audio server or session bus of
# the user's, in its own session so everything it starts can be stopped.
#
#   bash isolate-linux.sh node session.mjs <drive> ...
#
# MEDIAFLICK_VERIFY_HARD_TIMEOUT (seconds, default 900) bounds the whole run.
# The child learns its display from MEDIAFLICK_VERIFY_ISOLATION and checks it
# (and MEDIAFLICK_VERIFY_XVFB_PID) before launching anything.
set -euo pipefail

if [[ -n "${MEDIAFLICK_VERIFY_ISOLATION:-}" ]]; then
  echo "isolate-linux: already inside a verify-mediaflick isolation wrapper; refusing to nest." >&2
  exit 1
fi
if [[ $# -eq 0 ]]; then
  echo "isolate-linux: no command given." >&2
  exit 2
fi
if ! command -v Xvfb >/dev/null; then
  echo "isolate-linux: Xvfb is required (Arch: xorg-server-xvfb, Debian/Ubuntu: xvfb, Fedora: xorg-x11-server-Xvfb)." >&2
  exit 1
fi
timeout_s="${MEDIAFLICK_VERIFY_HARD_TIMEOUT:-900}"
if ! [[ "$timeout_s" =~ ^[0-9]+$ ]] || (( timeout_s < 1 || timeout_s > 3600 )); then
  echo "isolate-linux: MEDIAFLICK_VERIFY_HARD_TIMEOUT must be 1..3600." >&2
  exit 2
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/mediaflick-verify-xvfb.XXXXXX")"
xvfb_pid=""
child=""
cleanup() {
  local status=$?
  if [[ -n "$child" ]] && kill -0 "$child" 2>/dev/null; then
    kill -TERM -- "-$child" 2>/dev/null || true
    sleep 2
    kill -KILL -- "-$child" 2>/dev/null || true
  fi
  if [[ -n "$xvfb_pid" ]]; then
    kill -TERM "$xvfb_pid" 2>/dev/null || true
    wait "$xvfb_pid" 2>/dev/null || true
  fi
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# -displayfd picks a free display number and reports it, so parallel runs and
# the user's own X server never collide. No TCP listener.
mkfifo "$work/display"
Xvfb -displayfd 3 -screen 0 1920x1080x24 -nolisten tcp -noreset 3>"$work/display" 2>"$work/xvfb.log" &
xvfb_pid=$!
if ! read -r -t 15 display_number <"$work/display"; then
  echo "isolate-linux: Xvfb did not start:" >&2
  cat "$work/xvfb.log" >&2 || true
  exit 1
fi

export DISPLAY=":$display_number"
export MEDIAFLICK_VERIFY_ISOLATION="linux-xvfb:$DISPLAY"
export MEDIAFLICK_VERIFY_XVFB_PID="$xvfb_pid"
export XDG_SESSION_TYPE=x11
unset WAYLAND_DISPLAY WAYLAND_SOCKET XAUTHORITY DBUS_SESSION_BUS_ADDRESS PULSE_SERVER PIPEWIRE_REMOTE
# The user's runtime dir holds their Wayland, PipeWire and Pulse sockets; a
# private one keeps the app off the user's screen and speakers by default.
mkdir -m 700 "$work/runtime"
export XDG_RUNTIME_DIR="$work/runtime"

# A private session bus when available; otherwise none at all. Either way the
# app cannot reach the user's notifications, portals or keyring.
runner=()
if command -v dbus-run-session >/dev/null; then runner=(dbus-run-session --); fi

# setsid makes the child a session and process-group leader, so cleanup can
# stop the whole group (the app and CEF's helpers) with one signal.
setsid timeout --kill-after=10 "$timeout_s" "${runner[@]}" "$@" &
child=$!
set +e
wait "$child"
status=$?
set -e
if (( status == 124 )); then echo "isolate-linux: timed out after ${timeout_s} s." >&2; fi
exit "$status"
