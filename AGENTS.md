# Project Context

This project is a mix of `jellyfin-desktop` and `jellyfin-mpv-shim`.

When working in this repository, look up both projects for reference through the repo-analysis sources in `.agent-source.json`:

- Use the `jellyfin-mpv-shim` source (`jellyfin/jellyfin-mpv-shim`) as the reference for the external mpv implementation.
- Use the `jellyfin-desktop` source (`jellyfin/jellyfin-desktop`) as the reference for the CEF integration.

Local checkouts are also available at `C:\Users\pho\Source\jellyfin-mpv-shim` and `C:\Users\pho\Source\jellyfin-desktop` if needed.

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

# Changelog

Location: `CHANGELOG.md`.

Sections under `## [Unreleased]`: `### Breaking Changes` (API or behavior changes requiring migration), `### Added`, `### Changed`, `### Fixed`, `### Removed`.

Rules:

- Every code, behavior, packaging, build/release automation, or user-facing documentation change needs a `CHANGELOG.md` entry under `## [Unreleased]` in the same change. Pure changelog edits and release housekeeping are exempt.
- Read the full `[Unreleased]` section before editing it. Append to existing subsections; never duplicate subsection headings.
- Released version sections (for example, `## [0.1.0]`) are immutable; never modify them except for an intentional release-note correction requested by the user.
- Use the project `/cl` prompt before a release to audit commits since the last tag and fill in any missing entries.

Attribution:

- Internal or issue-backed changes: `Fixed foo ([#123](https://github.com/<owner>/<repo>/issues/123))`
- External contributions: `Added feature X ([#456](https://github.com/<owner>/<repo>/pull/456) by [@username](https://github.com/username))`

# Releasing

Releases are changelog-driven and use the `Draft Release` GitHub Actions workflow.

1. Run `/cl` on the latest release branch/main commit and make sure `CHANGELOG.md` `[Unreleased]` is complete.
2. Manually start the `Draft Release` workflow and enter the desired version, with or without a leading `v`.
3. The workflow promotes `[Unreleased]` to `## [version] - YYYY-MM-DD`, opens a fresh `[Unreleased]` section, updates `Cargo.toml` and `Cargo.lock`, builds the Windows installer and zip artifacts, commits `Release vX.Y.Z`, tags that commit, and creates a draft GitHub release using the changelog section as release notes with the artifacts attached.
4. Review the draft release before publishing it.
