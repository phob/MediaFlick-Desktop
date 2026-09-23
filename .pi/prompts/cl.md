---
description: Audit changelog entries before release
---
Audit changelog coverage (the `CHANGELOG.md` `[Unreleased]` section plus fragments under `changelog.d/`) for all commits since the last release.

## Cost-aware execution

- If `dynamic_subagents` is available, delegate the complete Git-history inspection and changelog edit to one `openai-codex/gpt-5.6-luna` sub-agent with `thinking: medium` and only the repository tools it needs (`hypa_read`, `hypa_grep`, `hypa_shell`, and `edit`).
- Keep the active model's role limited to supplying the instructions, reviewing the resulting diff, and running final validation. Do not repeat the delegated audit unless its result is incomplete or unsupported by evidence.
- If sub-agents or that model are unavailable, perform the audit directly with the active model.

## Process

1. **Find the last release tag:**
   ```bash
   git tag --sort=-version:refname | head -1
   ```
   If there is no release tag, audit all commits that are relevant to the upcoming release.

2. **List commits since that tag:**
   ```bash
   git log <tag>..HEAD --oneline
   ```
   If there is no tag, use:
   ```bash
   git log --oneline
   ```

3. **Read the full `[Unreleased]` section in `CHANGELOG.md` and every fragment under `changelog.d/`.**

4. **For each commit, check:**
   - Skip pure changelog edits and release housekeeping.
   - Skip anything AI related and don't mention AI skills used
   - Determine whether the commit affects users, behavior, packaging, build/release automation, documentation that users rely on, or maintainer workflow.
   - Verify a matching entry exists under the correct subsection, either in `[Unreleased]` or in a `changelog.d/` fragment.
   - For issue-backed changes, prefer: `Fixed foo ([#123](https://github.com/<owner>/<repo>/issues/123))`.
   - For external contributions, prefer: `Added foo ([#456](https://github.com/<owner>/<repo>/pull/456) by [@username](https://github.com/username))`.

5. **Fix coverage through fragments:**
   - Add each missing entry as a new `changelog.d/` fragment with a descriptive filename and a random suffix (for example `fix-playback-resume-a7c92e.md`).
   - Move misfiled entries to the correct subsection within their fragment.
   - Leave `CHANGELOG.md` unchanged; fragments are folded into `[Unreleased]` only in the release-notes PR (see `AGENTS.md`).

6. **Report:**
   - Commits that were already covered.
   - Commits that needed new or changed entries.
   - Any commits intentionally skipped and why.

## Changelog Format Reference

Sections, in order:

- `### Breaking Changes` - API or behavior changes requiring migration
- `### Added` - New features and capabilities
- `### Changed` - Changes to existing behavior
- `### Fixed` - Bug fixes
- `### Removed` - Removed features or support
