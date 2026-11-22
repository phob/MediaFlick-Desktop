### Changed

- Audited every code comment across the codebase, removing tautological, stale, and workaround comments, recording non-obvious architectural and protocol decisions in `docs/adr/` (`0007`–`0016`), and tracking identified workarounds as GitHub issues (`#413`–`#419`).
- Added a repo-wide comment-banning lint ratchet (`scripts/ci/check_comments.py`) enforced in PR checks and a `/delete-comment` skill (`.claude/skills/delete-comment/SKILL.md`) for repeatable comment audits.
