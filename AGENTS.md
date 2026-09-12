# MediaFlick Desktop agent guide

## Scope and completion

- Carry the requested change through implementation, affected contracts, appropriate validation, and fixes for failures it introduces. A first implementation is not the stopping point when verification or integration remains.
- Make routine implementation choices and run local validation without asking for approval at each step. Preserve unrelated work. Ask when a missing product decision materially changes the outcome or an action exceeds the authorized scope.
- Finish when the requested behavior is implemented and the relevant checks pass, or report a concrete blocker and the verification still outstanding. Expand scope only when necessary to complete the request.

## Context on demand

- `README.md` describes product behavior; `BUILDING.md` covers build, toolchain, and packaging work. Read the sections relevant to the task and reuse context already gathered.
- Treat `justfile`, `Cargo.toml`, `ui/package.json`, `global.json`, and `.github/workflows/ci.yml` as the authoritative commands and versions.
- `plugin/README.md` is the reference for Companion API, credentials, provider calls, caching, and capability discovery changes.

## Architecture and ownership

- `src/` owns the native app, CEF shell, local persistence, Jellyfin client, playback policy, and player adapters.
- `ui/` owns the embedded React interface. It talks to the native app through the typed API in `src/shell/cef/api/`; it must not call Jellyfin or provider services directly.
- `plugin/` is the optional Jellyfin Companion. It owns administrator credentials and fixed-origin calls to Seerr, Sonarr, Radarr, MDBList, and TMDB. Never expose service addresses, API keys, bearer tokens, or a generic proxy to Desktop.
- Keep policy in its owning domain module and protocol details in adapters. For example, segment-skip decisions belong in `src/playback/segments.rs`; mpv and MPC-HC should only translate actions into backend commands.
- Contract changes must be carried through every affected boundary in the same change: Rust request or response types, `ui/src/lib/api.ts`, UI callers and tests, Companion models and tests, and contract fixtures.
- `build.rs` embeds the UI bundle. A normal Cargo build may invoke pnpm. Use `MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1` only when a current UI bundle already exists and the task does not need integrated bundle verification.

## UI stack and components

- The embedded UI uses React 19, TypeScript, Vite, and pnpm. React Router handles navigation, TanStack Query handles asynchronous data and caching, and TanStack Virtual handles virtualized lists. Check `ui/package.json` for current versions and commands.
- Use the existing shadcn/ui components in `ui/src/components/ui/`, imported through `@/components/ui/*`. Interactive primitives use Radix UI. `ui/components.json` records the shadcn configuration (`new-york`, TSX, CSS variables, and Lucide icons).
- Before adding or changing UI controls, inspect the shared component and nearby callers. Reuse or extend those components instead of recreating available controls with raw HTML, custom interaction logic, or another component library. If a suitable control is missing, add an existing shadcn/ui component and any required dependencies when compatible with the app's stack, then adapt it to the app's theme in the shared UI directory. Prefer established shadcn controls over inventing equivalent controls from scratch.
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

Exercise built-in libmpv, external mpv, and MPC-HC where shared behavior changes. Keep access tokens in authentication headers and sanitized logs. MPC-HC URL authentication is the explicit adapter exception because it cannot attach request headers.

If a backend or real Jellyfin session is unavailable, complete applicable automated checks and report the specific runtime verification still outstanding.

## Persistence and compatibility

- `settings.json`, `accounts.json`, `collections.json`, playback preferences, and custom posters contain non-rebuildable user intent. Write them through the existing atomic save, backup, validation, and account-isolation paths.
- Catalog data in `library.db` and collection snapshots are rebuildable caches. Prefer schema recreation over speculative pre-1.0 migration code. Preserve the signed-in session through the existing database recreation path, and preserve account-owned preferences and posters outside the cache.
- Reject unsupported durable formats without rewriting or moving a valid newer file. Do not add future-version wrappers, unknown-variant preservation, or one-value configuration enums until a real second format or behavior exists.
- Never remove input validation, request bounds, fixed-host checks, path containment, cancellation, or secret redaction as a simplification.

## Implementation rules

- Shared media features must cover Movies and Series wherever the behavior applies. Keep media-specific behavior scoped to its domain and explain exclusions when relevant.
- Every Settings shelf must use the app's Save, Reset, and Discard workflow. Any change within a shelf must expose those controls instead of saving immediately.
- Keep Rust warnings and Clippy lints at error severity. Do not add lint suppressions to land a change.
- In Rust, release lock guards before networking, callbacks, logging, sleeps, or unrelated work.
- Route recoverable Rust failures through existing error boundaries. Do not add `unwrap`, `expect`, `todo!`, or `unimplemented!` to production code.
- When fixing lints at DTO or FFI boundaries, preserve external contracts; use a clearer internal type or ownership boundary where needed.
- Do not edit generated output under `build/`, `dist/`, `ui/dist/`, `plugin/bin/`, or `plugin/obj/`.

## Validation

Choose checks that establish the changed behavior and cover the affected boundaries. Add a focused regression test for non-trivial behavior, preferably through the public or user-visible path. The standard component checks are:

| Affected area | Local checks |
| --- | --- |
| Rust source, tests, or Cargo configuration | `just rust-quality`, `just test` |
| UI source, tests, dependencies, or configuration | `pnpm --dir ui lint`, `pnpm --dir ui test`, `pnpm --dir ui build` |
| Companion source, tests, dependencies, or configuration | `just plugin-test`, `just plugin` |
| Build, packaging, or workflow changes | Checks for the affected components and the relevant build or packaging command from `BUILDING.md` or the workflow |
| Documentation or agent instructions only | Review accuracy and links; no application build or test suites required |
| Any file edits | `git diff --check` |

Use focused checks during iteration. Run the full component checks for changes with broad impact or when focused checks cannot establish correctness; contract changes require checks for every affected side. Repeat checks only after relevant edits or when failures or unresolved concerns justify it. Report what was checked and any material gaps.

Use the recipes above for local validation. CI is defined by `.github/workflows/ci.yml` and additionally uses `--locked` for Cargo commands; reproducing a CI failure requires its exact command and environment. A passing local recipe does not require a second run solely to add `--locked`.

After changes to deployable Companion code or assets, run `just plugin-deploy` as the final step after validation and other work. Its publish dependency satisfies `just plugin` above. Documentation-only, test-only, and review tasks do not require deployment.

## Changelog and releases

- Every code, behavior, packaging, build, release-automation, or user-facing documentation change needs one entry under the matching subsection of `CHANGELOG.md` `[Unreleased]`. Pure changelog edits and release housekeeping are exempt.
- Read the full `[Unreleased]` section before editing it. Reuse existing subsection headings and never modify released sections unless the user requests a release-note correction.
- Release behavior is defined by `.github/workflows/draft-release.yml`; plugin release behavior is defined by `.github/workflows/plugin-release.yml`. Do not duplicate or bypass those workflows without a concrete reason.
