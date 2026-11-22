# 0001. Built-in player is in-process, dynamically loaded libmpv, chosen at startup

Status: Accepted (2026-11-22). Decided in `a4d76905` (Windows, 2026-08-28),
`b7b93123` (Linux, 2026-09-13) and `695e8553` (Linux default, 2026-10-10).

## Context

MediaFlick serves two audiences: users who want playback with no setup, and
power users whose own mpv carries `mpv.conf`, shaders, scripts and SVP. Requiring
a separately installed mpv failed the first group; dropping external mpv would
fail the second.

## Decision

The default player on Windows and Linux is a bundled libmpv loaded in-process at
runtime (`libloading`), not linked at build time. External mpv stays a supported
backend and is the default on macOS, which is an untested alpha. An explicit
saved backend always wins, and a saved `mpv_path` without one selects external
mpv, so an upgrade never bypasses an existing power-user setup.

The built-in runtime starts with `config=no` and `load-scripts=no`, so the
bundled player behaves the same whatever external mpv files a user has. SVP 4
on Windows is the one exception: when MediaFlick detects it, the runtime loads
scripts and switches hardware decoding for SVP.

Switching to or from the built-in player takes effect after a restart, because
the backend selects the native window and CEF composition model
([0002](0002-native-video-composition.md)). Switching between external mpv
paths rebuilds the player in place.

## Rejected alternatives

- Linking libmpv at build time: a missing or incompatible library would stop
  MediaFlick from launching even for a user who selected external mpv. Loaded
  at runtime, it is an ordinary backend startup error.
- Rebuilding the window and composition model when the backend changes while
  running: CEF's windowless rendering setting and, on Linux, its X11 and
  GPU-compositing switches are fixed when CEF initializes, so a switch would
  need a new process anyway.
- Making the built-in player honor the user's mpv configuration and scripts:
  it would make zero-setup playback depend on files MediaFlick does not
  control. External mpv is the path for full mpv customization.

## Consequences

- A native crash or deadlock in libmpv takes down the app. External mpv remains
  the isolation fallback. The runtime is owned by the controller thread and no
  libmpv handle crosses threads.
- Shutdown can block inside `mpv_terminate_destroy`. If that is seen in
  production, move libmpv into a small bundled helper process; do not force
  thread termination.
- Releases must ship the libmpv runtime with its licence notices and
  corresponding sources (`distribution/libmpv/*/README.md`).
