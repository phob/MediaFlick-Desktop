# MediaFlick Desktop agent guide

## Scope and completion

- Carry the requested change through implementation, affected contracts, appropriate validation, and fixes for failures it introduces. A first implementation is not the stopping point when verification or integration remains.
- Make routine implementation choices and run local validation without asking for approval at each step. Preserve unrelated work. Ask when a missing product decision materially changes the outcome or an action exceeds the authorized scope.
- Finish when the requested behavior is implemented and the relevant checks pass, or report a concrete blocker and the verification still outstanding. Expand scope only when necessary to complete the request.
- One request is one PR unless the operator asks to split it.

## Context on demand

- `README.md` describes product behavior; `BUILDING.md` covers build, toolchain, and packaging work. Read the sections relevant to the task and reuse context already gathered.
- Treat `justfile`, `Cargo.toml`, `ui/package.json`, `global.json`, and `.github/workflows/ci.yml` as the authoritative commands and versions.
- `plugin/README.md` is the reference for Companion API, credentials, provider calls, caching, and capability discovery changes.

## Product decisions and supported targets

- MediaFlick is a desktop app only: Windows, macOS and Linux.
- macOS is an untested alpha, proven only on GitHub-hosted arm64 VMs without a GPU (external mpv, software video). Never claim macOS works; report macOS runtime verification as outstanding.
- Segment-skip `Never` still shows the Skip button and only never skips by itself; `Don't show` hides that segment type.
- On Linux, playback targets Vulkan rendering and NVDEC hardware decoding.
- The desktop supports Jellyfin 10.11.0 and newer (`DESKTOP_MINIMUM` in `src/jellyfin/version.rs`). The Companion needs Jellyfin 12.0.0 (`MINIMUM_SERVER` in `src/companion/mod.rs`).

## Rules the checks enforce

Each rule below fails the named check; read the check's message for the fix. `just check-pr` runs them all on Linux or macOS. Every other rule in this file is judgment that no check catches, so review holds it.

| Rule | Enforced by |
| --- | --- |
| Rust warnings are errors; production code has no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` (tests may unwrap) | `[lints]` in `Cargo.toml`, `clippy.toml`, Clippy `-D warnings` (`just clippy`, CI `Clippy`, `Clippy (<target> target)`) |
| Rust is rustfmt-formatted | `just fmt-check` (CI `Check Rust formatting`) |
| Durable user files are written through `preferences::store::atomic_write` or their owning store, never plain `std::fs::write` | Clippy `disallowed-methods` in `clippy.toml` |
| An error is never turned into a default with `Result::unwrap_or_default`: propagate it, or match the error you expect and say why a default is correct | Clippy `disallowed-methods` in `clippy.toml` |
| `settings.json` is read only before the services open and written at startup only for command-line overrides; runtime code goes through `PreferencesService` | Clippy `disallowed-methods` (`AppSettings::load_at_startup`, `AppSettings::apply_launch_overrides`) |
| Jellyfin answers come back from `SessionScope::fetch` as `Scoped<T>` and open only through `Session::confirm` or `Live::open`, so an answer that outlived an account switch reaches neither the UI nor the cache; account-scoped writers take `&Live`; a request whose account changed answers 409 with `accountChanged: true` (`ApiError::AccountChanged`) | The compiler (`src/jellyfin/session.rs`) |
| Each API module declares its routes with `routes!` (`src/shell/cef/api/routes.rs`) next to their handlers: method, path, request body and answer. Bodies and answers are named Rust types in `wire_types!` (`src/shell/cef/api/wire.rs`); a handler returns its route's answer type and decodes its body with `Body::decode`. Run `just wire-types` after changing one to regenerate `ui/src/lib/api/wire.gen.ts`, the per-module files in `ui/src/lib/api/wire/` and the `Routes` map `ui/src/lib/api/client.ts` calls through, and alias the generated type in `ui/src/lib/api/types/` instead of writing a TypeScript copy | `cargo test` `generated_typescript_is_current` (`just test`, CI `Test`) |
| The count of untyped API bodies only falls | `cargo test` `untyped_responses_only_shrink` (CI `Check that the untyped-body count runs`), and `Guard integrity` for its `UNTYPED_*` ratchets |
| Query keys come only from `queryKeys` in `ui/src/lib/query-client.ts` | TypeScript (`pnpm --dir ui build`, CI `Type-check and build UI`) |
| Media kind translations (Jellyfin, Seerr, collections, calendar) go through `ui/src/lib/kinds.ts` | Oxlint `ui/no-inline-kind-translation` (`pnpm --dir ui lint`, CI `Lint UI`) |
| Raw `button`, `input`, `select` and `textarea` elements belong only in `ui/src/components/ui/`; an unstyled control uses `Pressable` | Oxlint `react/forbid-elements` |
| No `any` in UI code | Oxlint `typescript/no-explicit-any` |
| No new non-null assertions, empty fallbacks for query data, or raw palette colors (use a theme token from `ui/src/app.css`) | UI ratchet `scripts/ci/check_ui_ratchet.py` (PR scripts `Check UI rule ratchets`) |
| No unused UI files, exports or dependencies | knip (`pnpm --dir ui knip`, CI `Find unused UI code`) |
| `pnpm` leaves `ui/pnpm-lock.yaml` unchanged | CI `Check that pnpm left the lockfile unchanged` |
| No code copied between files; give it one owner | `scripts/ci/check_clones.py` (CI `Check for code copied between files`) |
| A source file over 1,000 lines may not grow | `scripts/ci/check_file_size.py` (PR scripts `Check file sizes`) |
| A change to `src/`, `ui/src/` or `plugin/` carries a `changelog.d/` fragment; only the `no-changelog` PR label waives it | `scripts/ci/check_changelog_fragment.py` (PR scripts `Check changelog fragment`) |
| Every "Proven by" claim in the verify feature map names an existing drive and only evidence files that drive writes, never a run id | `scripts/ci/check_feature_map.py` (PR scripts `Check verify feature map proofs`) |
| A `library.db` schema change bumps `SCHEMA_VERSION` and appends exactly one step to `MIGRATIONS` | The compiler (`src/library/db/migrate.rs`) |
| Every released `library.db` schema (`fixtures/library-db/released/schema-<N>.sql`, never edited) opens at the structure of a fresh install and keeps every row of `rows-<N>.sql` value for value; a failed or foreign-key-breaking migration leaves the file as it was; a newer schema is refused untouched | `cargo test` `library::db::tests` (`just test`, CI `Test`) |
| The JSON files 0.2.0 wrote (`fixtures/preferences/released/0.2.0/`) load without recovery, keep their values and survive a save | `cargo test` `preferences::released_formats` |
| A version is published only with its `library.db` schema frozen in `fixtures/library-db/released/` | `scripts/prepare-release.py` (`--transition` in PR scripts `Check the release.toml change`, `--check` in the release workflow) |
| `release.toml` names the `Cargo.toml` version; a new version ships first as a prerelease with its release notes prepared; versions never go back, and Latest never becomes a prerelease again | `scripts/prepare-release.py --transition` (PR scripts `Check the release.toml change`) |
| No lint suppression, relaxed lint level or Clippy, Oxlint or analyzer setting, raised `guard-ratchet` count, or other change to guard code without the `guards-changed` label | `Guard integrity` (`scripts/ci/check_guards.py`); see [Validation](#validation) |
| Workflows are valid | actionlint (PR scripts `Lint workflows`) |

## Architecture and ownership

- `src/` owns the native app, CEF shell, local persistence, Jellyfin client, playback policy, and player adapters.
- `ui/` owns the embedded React interface. It talks to the native app through the typed API in `src/shell/cef/api/`; it must not call Jellyfin or provider services directly.
- `plugin/` is the optional Jellyfin Companion. It owns administrator credentials and fixed-origin calls to Seerr, Sonarr, Radarr, MDBList, and TMDB. Never expose service addresses, API keys, bearer tokens, or a generic proxy to Desktop.
- Keep policy in its owning domain module and protocol details in adapters. For example, segment-skip decisions belong in `src/playback/segments.rs`; the mpv adapter should only translate actions into backend commands.
- Contract changes must be carried through every affected boundary in the same change: Rust request or response types, `ui/src/lib/api.ts`, UI callers and tests, Companion models and tests, and contract fixtures. The generated wire types (table above) cover only the Desktop API; carrying Companion models and fixtures is up to you.
- `build.rs` embeds the UI bundle. A normal Cargo build may invoke pnpm. Use `MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1` only when a current UI bundle already exists and the task does not need integrated bundle verification.

## UI stack and components

- The embedded UI uses React 19, TypeScript, Vite, and pnpm. React Router handles navigation, TanStack Query handles asynchronous data and caching, and TanStack Virtual handles virtualized lists. Check `ui/package.json` for current versions and commands.
- Use the existing shadcn/ui components in `ui/src/components/ui/`, imported through `@/components/ui/*`. Interactive primitives use Radix UI. `ui/components.json` records the shadcn configuration (`new-york`, TSX, CSS variables, and Lucide icons).
- Before adding or changing UI controls, inspect the shared component and nearby callers. Reuse or extend those components instead of recreating available controls with raw HTML, custom interaction logic, or another component library. If a suitable control is missing, add an existing shadcn/ui component and any required dependencies when compatible with the app's stack, then adapt it to the app's theme in the shared UI directory.
- Settings must use the shared Button, Input, Label, Select, Switch, Checkbox, and Slider components wherever applicable. Preserve accessible labels, keyboard operation, focus states, and disabled states. Compose shelves with the existing `SettingsSaveBar`, `SettingsDraftGuard`, and draft hooks to retain Save, Reset, and Discard behavior.
- Styling uses Tailwind CSS 4 with CSS-first theme tokens and the app's custom appearance rules in `ui/src/app.css`. Preserve those tokens and appearance settings when using shadcn components; do not replace them with stock shadcn styling or hardcoded colors. Use `cn` from `@/lib/utils` to merge conditional classes.
- Use `lucide-react` for icons and Sonner for toast notifications. UI validation uses Oxlint, Node's test runner, and Vitest with React Testing Library; run the applicable checks listed below.

## Playback invariants

Before changing playback startup, resume behavior, playstate reporting, media-segment handling, or mpv IPC, trace the end-to-end user path and inspect every supported backend affected by the change.

Preserve these known-good resume rules unless logs from a real Jellyfin session justify replacing them:

- Keep one persistent mpv IPC command writer during playback.
- Apply resume after `file-loaded`; do not use mpv `loadfile` `start` or a URL `#t=` fragment.
- Hold the reported Jellyfin position until the delayed startup seek reaches its target range.
- Do not send a startup `pause=false` command merely to compensate for load timing.
- Do not clone event-pipe writes or reopen a Windows pipe for each command.

These rules apply to built-in libmpv on every platform and to external mpv. Exercise both runtimes where shared behavior changes. Keep access tokens in authentication headers and sanitized logs.

If a backend or real Jellyfin session is unavailable, complete applicable automated checks and report the specific runtime verification still outstanding.

## Persistence and compatibility

- `library.db` has real users at schema 23, which MediaFlick 0.2.0 shipped, and no release may destroy their database. From schema 23 on, a schema change bumps `SCHEMA_VERSION` and appends one step to `MIGRATIONS` in `src/library/db/migrate.rs`; a step that has shipped is never edited, reordered or removed, and the next step fixes it. Only development schemas below 23 are recreated.
- Upgrades from schema 23 only add (expand-only): new tables, nullable or defaulted columns, indexes and triggers. Released builds open a newer database and run their own SQL against it (0.2.0 only logs a warning), so never rename, drop or tighten anything an older release reads or writes.
- Every table rebuild and every data drop or rewrite in a migration needs the operator's approval, in its own PR that states its real cost to users (a resync, an emptied Manual collection, spent provider quota, a sign-in). This includes the caches.
- Newer code accepts rows an older release wrote: with a new column at its default where that release inserted the row, and stale where it updated it (0.2.0 upserts with `ON CONFLICT DO UPDATE`, which leaves columns it does not know as they were). Keep a derived column current with a trigger, which an older release fires too. A new account-scoped table keys its rows by server and user, as the snapshot tables do: an older release's account switch does not clear tables it does not know.
- A migration runs in one transaction with foreign keys off and is checked with `foreign_key_check` and `quick_check` before it commits. A failed migration leaves `library.db` as it was and keeps the services down; never fall back to recreating the database. There is no automatic backup, so a migration that commits wrong data is the remaining risk: prove each step against the frozen fixtures (`fixtures/library-db/released/`) and with `just upgrade-e2e-0-2-0`.
- User intent (`settings.json`, `accounts.json`, `collections.json`, `playback-preferences.json`, `reader.json`, posters) must carry over from 0.1.6 and from every release since. Write these files through the existing atomic save, backup, validation, and account-isolation paths; the `upgrade-0-1-6` and `upgrade-0-2-0` verify drives exercise the carry-over.
- A JSON format changes by adding a field with `#[serde(default)]`, without a `version` bump; bump `version` only when an existing field changes shape or meaning. An older release refuses to start on a file with a field or enum value its strict types do not know, and drops unknown fields from its lenient ones on its next save: check where the new field lands and say in the changelog what an older release does with the file.
- `custom-art/` holds only collection posters that `collections.json` names by `customPosterId`; a new kind of upload gets its own directory, because 0.2.0's startup sweep deletes any other file there after 7 days.
- Reject unsupported durable formats without rewriting or moving a valid newer file. Do not add future-version wrappers, unknown-variant preservation, or one-value configuration enums until a real second format or behavior exists.
- Never remove input validation, request bounds, fixed-host checks, path containment, cancellation, or secret redaction as a simplification.

## Implementation rules

- Shared media features must cover Movies and Series wherever the behavior applies. Keep media-specific behavior scoped to its domain and explain exclusions when relevant.
- Every Settings shelf must use the app's Save, Reset, and Discard workflow. Any change within a shelf must expose those controls instead of saving immediately.
- In Rust, release lock guards before networking, callbacks, logging, sleeps, or unrelated work. Clippy catches only a guard held across an `.await` or past its last use.
- Route recoverable Rust failures through existing error boundaries; the lints in the table forbid the panicking shortcuts.
- When fixing lints at DTO or FFI boundaries, preserve external contracts; use a clearer internal type or ownership boundary where needed.
- Do not edit generated output under `build/`, `dist/`, `ui/dist/`, `plugin/bin/`, or `plugin/obj/`.

## Validation

Choose checks that establish the changed behavior and cover the affected boundaries.

When adding tests, what matters is where the expected result comes from. A test whose expectations were read off the code it checks passes whatever that code does and proves nothing.

- Verify behavior a user or caller sees end to end, by driving the real path: the running app, a loopback Jellyfin or Companion server, or a real mpv runtime. Run the drives that cover the changed behavior (`.claude/skills/verify-mediaflick/features/README.md` maps features to drives), not every drive. A change that only alters presentation, such as a label, font or spacing, needs a screenshot of each affected page, not functional drives.
- Every end-to-end test must end by producing a verifiable, repeatable artifact that a reviewer can inspect or regenerate, such as a log, screenshot, recording, or JSON report.
- A new drive, or a new check in an existing drive, ships with a recorded negative control: the check fails on a build with the change reverted, or, for behavior that already shipped, with that behavior broken on purpose (`just verify <drive> --negative-control NAME`; see the verify-mediaflick skill's SKILL.md).
- Isolated tests (Rust unit tests, Vitest, Companion tests) are for pure domain logic with a stated contract, such as playback policy in `src/playback/`, parsers, filters, sorting and kind mappings. Isolate that logic by its inputs; do not mock modules. For each isolated test:
  - Take the expected values from a source independent of the implementation: a rule the operator approved, a real Jellyfin, Companion or mpv log or response, a reproduced bug, or an invariant. Name the source in the test or the PR description.
  - First write down every way the logic could fail (in the plan or PR description). For new logic, then write the tests, run them and see each fail on its assertion for the stated reason, and only then write the code. A compile error does not count: give a new function a body that returns a wrong value until its tests fail. Never write isolated tests after new code they cover, except a test that reproduces a fault found during implementation and fails before the fix.
  - A test for existing behavior must catch the fault it names: break the code that way, for example with `cargo mutants`, and see the test fail.
  - Where a rule holds for every input, check it as an invariant over many generated inputs instead of a few hand-picked examples.
- A bug fix starts with a test that reproduces the bug and fails, end to end or isolated by the rules above.
- When a test fails, fix the code or add a test case. Do not rewrite an existing test's scenario so that it passes.
- Do not change an existing test's expectations in the same change as the code it covers unless the operator asked for that behavior change; say so in the PR description. Never weaken, delete or skip a failing test to make a change pass.
- If neither an end-to-end path nor an isolated contract exists for the changed behavior, report that gap instead of writing tests that mirror the code.
- Existing unit, component, and Companion tests remain part of the checks below. Keep them passing and update them for contract changes.

The standard component checks are:

| Affected area | Local checks |
| --- | --- |
| Rust source, tests, or Cargo configuration | `just rust-quality`, `just test` |
| UI source, tests, dependencies, or configuration | `pnpm --dir ui lint`, `pnpm --dir ui knip`, `pnpm --dir ui test`, `pnpm --dir ui build`, `python3 scripts/ci/check_ui_ratchet.py origin/main` |
| Companion source, tests, dependencies, or configuration | `just plugin-test`, `just plugin` |
| Build, packaging, or workflow changes | Checks for the affected components and the relevant build or packaging command from `BUILDING.md` or the workflow |
| The verify-mediaflick skill's feature map or drives | `python3 scripts/ci/check_feature_map.py` |
| Documentation or agent instructions only | Review accuracy and links; no application build or test suites required |
| Any file edits | `git diff --check` |

Use focused checks during iteration. Run the full component checks for changes with broad impact or when focused checks cannot establish correctness; contract changes require checks for every affected side. Repeat checks only after relevant edits or when failures or unresolved concerns justify it. Report what was checked and any material gaps.

Use the recipes above for local validation. CI is defined by `.github/workflows/ci.yml` and `.github/workflows/pr-checks.yml` and additionally uses `--locked` for Cargo commands; reproducing a CI failure requires its exact command and environment. A passing local recipe does not require a second run solely to add `--locked`. `just check-pr` runs every PR check locally on Linux or macOS before you open or update a PR; `just clippy-target <triple>` runs one cross-target Clippy.

The `Guard integrity` check fails a change that relaxes a lint level or a Clippy or Oxlint setting, adds a lint suppression, or raises a `guard-ratchet` count, and any change to guard code: `scripts/ci/`, the workflows, the check recipes, `.cargo/config*`, the UI's check scripts in `ui/package.json`, TypeScript, MSBuild and analyzer warning settings, the UI's own lint rules and ratchet (`ui/lint/`), the knip configuration, and the count of untyped API bodies with its `UNTYPED_*` ratchets (`src/shell/cef/api/wire/untyped_count.rs`, and in `wire.rs` its `mod` declaration and `ApiResponse::untyped`). The clone gate's `MAX_CLONES` and the file-size `LIMIT` are `guard-ratchet` counts. Change the code instead. The `guards-changed` label accepts a relaxation; add it only when the operator asked for that relaxation, and say why in the PR body.

After changes to deployable Companion code or assets, run `just plugin-deploy` as the final step after validation and other work. Its publish dependency satisfies `just plugin` above. Documentation-only, test-only, and review tasks do not require deployment.

## Changelog and releases

- Every code, behavior, packaging, build, release-automation, or user-facing documentation change needs an entry in a Markdown fragment under `changelog.d/`, committed with the change. Pure changelog edits and release housekeeping are exempt; the table above names the check and its waiver.
- Write or refresh the change's fragment last, from the end-to-end evidence, so it states what the change was shown to do. A fragment may exist before the PR does. Use a descriptive filename with a random suffix, such as `fix-playback-resume-a7c92e.md`; no PR number or later renaming is required. Keep one fragment per change. Do not maintain a shared fragment index.
- Write concise, user-facing entries under the matching existing subsection headings: `Breaking Changes`, `Added`, `Changed`, `Fixed`, or `Removed` (using `###` Markdown headings).
- Keep the existing `CHANGELOG.md` `[Unreleased]` section as the baseline for the upcoming release. Regular implementation work must use fragments instead of editing `CHANGELOG.md`; edit that file only for release consolidation or an explicitly requested correction.
- Before releasing, consolidate all merged fragments into `[Unreleased]` in a release-notes PR. Read the full section, preserve the existing baseline's relevant changes, combine overlapping entries, and remove notes superseded by later work. Delete consumed fragments in the same PR. Then run `python3 scripts/prepare-release.py --version <version> --notes-out <file>` in that PR and commit its changes to `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`, and `release.toml`. Never modify released sections unless the user requests a release-note correction.
- A merged change to `release.toml` is the release, so a PR merge is the operator's approval; never start one any other way. Merging the release-notes PR, which moves `release.toml` to a new version on the `prerelease` channel, builds, tags, and publishes that version as a GitHub prerelease, which the updater and flick.media do not offer. A later PR that changes only `channel` to `"latest"` makes the published prerelease Latest without a rebuild. Open that PR only when the operator asks for it. A merge that leaves `release.toml` unchanged releases nothing, and the PR checks refuse a version that differs from `Cargo.toml`, moves backward, starts as Latest, or returns from Latest to prerelease.
- Release behavior is defined by `.github/workflows/release.yml`, which decides from `release.toml`, and `.github/workflows/draft-release.yml` (Build Release), which builds and publishes; run the latter by hand only to retry a failed build of the version `release.toml` names, or for a `test_draft`. Plugin release behavior is defined by `.github/workflows/plugin-release.yml`. Do not duplicate or bypass those workflows without a concrete reason.
