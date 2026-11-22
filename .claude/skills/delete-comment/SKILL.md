---
name: delete-comment
description: Audit remaining code comments into Tautology, Excuse, Stale, or Decision Record, delete them (filing issues for excuses and moving decision records to docs/adr/), and ratchet the comment lint count down. Use for /delete-comment.
disable-model-invocation: true
---

# Delete comments

Audit code comments in this repository (or a scoped path), classify each comment, apply its required action, and verify the comment-banning ratchet drops and never rises.

## 1. Inventory remaining comments

Run the repository's comment linter in list mode to get every remaining non-exempt comment block and line count:

```sh
python3 scripts/ci/check_comments.py --list --json [PATH...]
```

The checker scans tracked `.rs .ts .tsx .js .mjs .cs .css .sh .ps1 .py` files and automatically exempts only:
- Rust `// SAFETY:` blocks (required by `forbid(clippy::undocumented_unsafe_blocks)` in `Cargo.toml`)
- Generated files (`ui/src/lib/api/wire.gen.ts`, `ui/src/lib/api/wire/*.gen.ts`, `distribution/libarchive-wasm/bundle/worker-bundle.js`)
- Bundler/linter directives required by build or CI (`/* @vite-ignore */`, `# shellcheck disable=...`)
- License/copyright attribution blocks (`SPDX-License-Identifier`, `Copyright`, `CC0-1.0`)
- Shebangs (`#!`) and Python runtime strings/docstrings

If the scope is large (> 100 blocks), split the JSON inventory into contiguous file slices and audit slices in parallel; never classify a comment without reading its surrounding code.

## 2. Categorize each comment

Assign every comment block exactly one category:

| Category | Definition | Action |
| --- | --- | --- |
| **Tautology** | Repeats what is already visible from the code (step narration, restated names/types/tests, banners, obvious docstrings). | Delete the comment. |
| **Stale** | References removed code, outdated behavior, old architecture, or inaccurate details. | Delete the comment. |
| **Excuse** | Rationalizes a hacky implementation, workaround, timing sleep, hardcoded duplication, or lint suppression so debt survives in code. | Delete the comment and file a GitHub issue describing the underlying flaw and concrete fix. Group tightly related small workarounds in one subsystem when appropriate, and link filed issues. |
| **Decision Record** | Documents an important, non-obvious architectural, protocol, platform, or compatibility decision that someone reading the code would not infer. | Check `docs/adr/` first. If already covered, confirm no non-obvious detail is missing; otherwise add a numbered ADR in `docs/adr/` (following `docs/adr/README.md`) and update the index table. Then delete the comment from the code. |

## 3. Apply deletions and regenerate derived files

- Remove the categorized comments cleanly without altering code behavior or leaving double blank lines.
- If any Rust wire type doc comments changed, run `just wire-types` so `ui/src/lib/api/wire.gen.ts` and `ui/src/lib/api/wire/*.gen.ts` stay in sync.
- Run `just fmt` if any `.rs` file changed.

## 4. Validate

Run the comment ratchet and every affected component check:

```sh
python3 scripts/ci/check_comments.py origin/main
python3 scripts/ci/check_comments.py --list
```

Then run the applicable checks from `AGENTS.md` (`just rust-quality`, `just test`, `pnpm --dir ui lint`, `pnpm --dir ui knip`, `pnpm --dir ui test`, `pnpm --dir ui build`, `just plugin-test`, `python3 -m unittest discover -s scripts/ci -p 'test_*.py'`, `git diff --check`).

## 5. Report

Summarize:
- Counts by category (`Tautology`, `Stale`, `Excuse`, `Decision Record`)
- Issues filed for `Excuse` comments (with links)
- ADRs created or confirmed for `Decision Record` comments
- Lines of code shed in this pass and remaining non-exempt comment lines under the ratchet
