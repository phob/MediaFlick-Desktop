---
name: prune-comments
description: Audit every code comment in MediaFlick Desktop with a swarm of Comment Sicko workers and return a plan that splits prunable comments into delete-now and refactor-first. Use for /prune-comments, "find useless comments", or "which comments can we prune". Runs under poteto-mode and fans out parallel agents, so run it only when the user asks.
disable-model-invocation: true
---

# Prune comments

Run this under `/t3-pstack:poteto-mode`. An audit is that skill's Investigation playbook, and cleanup is its Refactoring playbook. The fan-out follows the **swarm** skill's four phases. Each worker is Comment Sicko from the **no-comments** skill, in audit mode.

`<plugin root>` is the t3-pstack directory that holds `skills/` and `agents/`. `<run>` is a plain name for this audit, such as `audit-2026-10-14`.

## Modes

- **Audit (default).** Requests to find, list, or plan are read-only. Workers edit no file. The deliverable is the plan in [Report](#report).
- **Cleanup.** Only after the user approves an audit's plan. See [Cleanup](#cleanup).

## Frame

```sh
node .claude/skills/prune-comments/scripts/inventory.mjs --run <run> [--slices 8] [-- <pathspec>...]
```

The script lexes every comment in tracked `.rs .ts .tsx .js .mjs .cs .css .sh .ps1 .py` files into blocks. A block is one comment or one run of adjacent whole-line comments of the same kind. The script also reads `#[allow]`, `#[expect]`, `#pragma warning disable`, and `[SuppressMessage]` as suppression blocks. It cuts the path-sorted files into contiguous slices of about equal block counts and writes `tmp/prune-comments/<run>/inventory.json` and `slice-<k>.todo.json`. `tmp/` is gitignored.

The lexer does not read comments inside embedded code, such as a heredoc body or JS passed to `page.evaluate` as a template string. Say so in the report when the scope holds such code.

The done predicate is `merge.mjs --run <run>` exiting 0.

## Fan out

Spawn one `delegate_task` per slice in one response, with `mode: "async"`, `role: "review"`, and the `swarm workers` target from the swarm skill. Workers share the main checkout. Each one writes only its own verdicts file. The brief:

> Read `<plugin root>/agents/comment-sicko.md` in full and act as it says, in audit mode. Then read `<repo>/.claude/skills/prune-comments/SKILL.md` and follow its Worker contract and Leash sections. Your input is `<repo>/tmp/prune-comments/<run>/slice-<k>.todo.json` at commit `<sha>`. Write only `<repo>/tmp/prune-comments/<run>/slice-<k>.verdicts.json`. Do not edit any other file.

## Worker contract

Give each block in the todo file exactly one verdict. Read the code around the block before judging it, and apply Comment Sicko's rules and checks. Do the `/how` and `/why` checks yourself with code reads, `git log -S`, and `git blame`. Spawn no subagents. Map its outcome to a verdict:

| Verdict | When | Extra fields |
| --- | --- | --- |
| `DELETE` | Kill, and no code change makes up for the loss. Narration, restated names, banners, dead code, stale notes. | none |
| `REFACTOR` | Kill and `MUST KILL`. The comment explains a surprise in our code that a code change would make obvious. | `symbol`: the exact guilty symbol. `fix`: `rename`, `extract`, `type`, `test`, `delete-dead-path`, `use-real-api`, or `rearchitect`. |
| `KEEP` | A keep-list exception, proven today on a live path. | `exception`: `license`, `foreign-constraint`, `lint-required`, `lint-rule-faulty`, `api-contract`, `issue-link`, or `prettier-ignore`. |
| `SKIP` | The block is not a comment, such as a URL in JSX text that the lexer misread. | none |

Every verdict also has `id` (from the todo file), `verdict`, and `why`, one sentence. A `foreign-constraint` `why` names the dependency, the behavior, and the evidence you read. Write the file as:

```json
{ "slice": 3, "commit": "<sha>", "verdicts": [{ "id": "src/app/cli.rs:12", "verdict": "DELETE", "why": "Restates the function name." }] }
```

Check it with `node <repo>/.claude/skills/prune-comments/scripts/merge.mjs --run <run> --slice <k>`. Fix every error and gap until it prints `slice <k> clean`. Report `PASS` with the verdict counts and one line for each refactor theme you saw, or `BLOCKED` with the reason.

## Leash

These repository facts settle verdicts that Comment Sicko's rules leave open.

- The Rust crate is a binary, so no Rust item is a public API. Nothing requires doc comments. Cargo has no `missing_docs` lint, and the Companion builds with `GenerateDocumentationFile` set to false.
- `Cargo.toml` denies `clippy::undocumented_unsafe_blocks`, so a `// SAFETY:` comment on an `unsafe` block is `KEEP` with `lint-required` when it states the real invariant.
- Doc comments on clap `Parser` fields in `src/app/cli.rs` are `--help` output, not comments. They are `SKIP`.
- `api-contract` covers only doc comments that state a wire fact the type cannot, such as a unit, range, or null meaning. They must sit on the Rust and UI contract (`src/shell/cef/api/`, `ui/src/lib/api.ts`, `ui/src/lib/api/`) or on Companion HTTP endpoints and DTOs in `plugin/`. A contract fact a type could carry is `REFACTOR` with `type`.
- A foreign dependency is anything whose source is not in this repository. Examples are Jellyfin, mpv, CEF, the OS APIs, the Seerr, Sonarr, Radarr, MDBList, TMDB, and Letterboxd services, toolchains and runtimes such as Cargo, .NET, Node, and ffmpeg, and every third-party package. A comment about one of them is `KEEP` only if the behavior still applies on a live path.
- A comment that guards one of the `AGENTS.md` playback invariants is `KEEP` when it names the mpv or Jellyfin behavior behind the rule. Otherwise it is `REFACTOR` with `test`.
- `AGENTS.md` forbids lint suppressions. Every suppression is `REFACTOR` unless the rule is proven faulty for that line.
- In verify drives and scripts, a comment that narrates a step is `DELETE`. Assertion and log strings document steps.
- A script header that states how to run the script is `DELETE` when a skill, `BUILDING.md`, or the `justfile` documents the same command. Otherwise it is `REFACTOR` with `use-real-api`, which means the script prints its own usage.

## Aggregate

Run `node .claude/skills/prune-comments/scripts/merge.mjs --run <run>`. It validates every verdict against the inventory, writes `report.json` and `report.md`, and exits 1 on any gap or error. If a slice fails, spawn a fresh worker for it with the failure named. A second failure is a gap in the report.

Then review the result as step 2 of the **no-comments** skill describes. Check every `KEEP` and `REFACTOR` and a sample of `DELETE`s from each slice against the code. Overturn misstated `MUST KILL` reasons and keeps without proof. Record each overturn.

## Report

Group the `REFACTOR` flags into themes, such as one guilty pattern repeated across files or one module whose comments all explain the same design debt. For each theme, give the fix kind, the files, the comment lines it removes, and whether it is one PR or several. Then reply with:

- The `report.md` totals table and its path.
- **Delete now.** Counts by area, with the noisiest files named. Report the Long column on its own, because design rationale hides in long deletes.
- **Refactor first.** The themes, ordered by comment lines removed per unit of risk.
- **Keep.** Counts by exception, with any keep you doubt.
- Overturns, gaps, and the lexer limits that applied.

## Cleanup

Start from an approved `report.json`. Rerun `inventory.mjs` with a new `--run`. A verdict still applies only where a block with the same path and text exists. Audit the rest again. Make one PR for the short `DELETE` verdicts and one reviewed PR for each area's long ones, then one Refactoring PR for each refactor theme. A refactor PR deletes its theme's comments in the same change. Finish every PR with `/no-comments` on its diff.
