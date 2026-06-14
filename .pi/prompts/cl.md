---
description: Audit changelog entries before release
---
Audit `CHANGELOG.md` entries for all commits since the last release.

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

3. **Read the full `[Unreleased]` section in `CHANGELOG.md`.**

4. **For each commit, check:**
   - Skip pure changelog edits and release housekeeping.
   - Determine whether the commit affects users, behavior, packaging, build/release automation, documentation that users rely on, or maintainer workflow.
   - Verify a matching changelog entry exists under the correct `[Unreleased]` subsection.
   - For issue-backed changes, prefer: `Fixed foo ([#123](https://github.com/<owner>/<repo>/issues/123))`.
   - For external contributions, prefer: `Added foo ([#456](https://github.com/<owner>/<repo>/pull/456) by [@username](https://github.com/username))`.

5. **Fix `CHANGELOG.md` directly:**
   - Add missing entries under `[Unreleased]`.
   - Move entries to the correct subsection when needed.
   - Do not edit released version sections.

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
