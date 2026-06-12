# Project Context

This project is a mix of `D:\users\pho\Documents\Source\jellyfin-desktop` and `D:\users\pho\Documents\Source\jellyfin-mpv-shim`.

When working in this repository, look up both projects for reference:

- Use `D:\users\pho\Documents\Source\jellyfin-mpv-shim` as the reference for the external mpv implementation.
- Use `D:\users\pho\Documents\Source\jellyfin-desktop` as the reference for the CEF integration.

# Playback Regression Guard

Before changing playback startup, resume behavior, Jellyfin playstate reporting, or mpv IPC code, read `docs/playback-regression-invariants.md`.

The resume/startup fix documented there is a known-good behavior from real Jellyfin Web and external mpv logs. Do not replace it with mpv `loadfile` `start`, URL `#t=` resume starts, per-command Windows pipe opens after `file-loaded`, cloned event-pipe command writes, or startup `pause=false` commands unless you are intentionally re-debugging that regression with logs.

# Commands

Use `just` for linting and testing. Configured commands:

- `just fmt`: format the Rust crate.
- `just fmt-check`: check Rust formatting.
- `just clippy`: run clippy with warnings denied.
- `just build`: build and stage the debug app into `./build`.
- `just release`: build and stage the release app into `./build`.
- `just run --url http://localhost:8096`: build and run the staged app.
- `just run-mpv`: run the external mpv binary.
- `just clean`: remove build artifacts.

There is not currently a dedicated `just` test recipe.
