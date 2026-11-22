# 0009. Account scoping, startup lifecycle, and single-writer persistence are enforced structurally

Status: Accepted (2026-11-22). Consolidated from codebase audit of `src/jellyfin/session.rs`, `src/app/`, `src/shell/locator.rs`, `src/preferences/`, and `ui/src/lib/`.

## Context

Users switch between Jellyfin accounts (often on the same server, where items share identical IDs), sign out mid-request, or delete local account data while background sync, batch card loaders, and CEF API threads are in flight. Startup also races concurrent CEF requests against SQLite initialization and settings persistence.

## Decision

- **Compiler-enforced session scoping.** Every account-scoped Jellyfin call goes through `SessionScope::fetch` returning `Scoped<T>`, which can only be opened through `Session::confirm` or `Live::open` (minted by `Session::commit_if_current` under `operation_gate`). `SessionScope::client()` is restricted to `pub(in crate::jellyfin)` for socket, remote-control, and playback paths that check scope manually. Because `Err(ApiError)` is unsealed, callers route failures through `Session::scoped_error`. `confirm` distinguishes `Unauthorized` (the account's own rejected token expired it) from `AccountChanged` (switch, sign-out, or deletion -> HTTP 409 `{ accountChanged: true }`, overriding route-specific 409s). On the first `Unauthorized`, `Session` pauses sync without wiping the cache and immediately pushes `shell.session_expired()` so best-effort callers that swallow errors cannot strand the UI on dead screens.
- **Multi-account shared item IDs and policy gating.** Two accounts on one Jellyfin server share item IDs, so `RatingsProvider` and `TechnicalProvider` never mark `accountChanged` 409 responses as checked and instead re-queue active IDs for the new account. `ReviewService` keys public Letterboxd caches by `(server_id, user_id, profile_id)` for isolation. Before a live policy read succeeds, `UserAccess::UNVERIFIED` fails closed (`restricted: true`, `administrator: false`), hiding Missing titles (when Jellyfin restricts the user's catalog) and admin setup until verified.
- **Journaled local account deletion.** Account deletion journals the pending deletion and signs out before clearing stores sequentially; a mid-way failure leaves the account signed out (requiring UI `status` invalidation on error) and resumes cleanup on launch. Pending deletions are drained both at startup and before `auth_login` / `quick_connect_start` so signing back into a previously deleting account cannot be wiped on the next launch.
- **Two-phase startup and latched failures.** `Services` splits construction (`open`) from launching background schedulers (`start`) so the shell publishes `Arc<Services>` before schedulers emit callbacks, and binds `PlaybackCoordinator` later (`attach_playback`) once browser state exists. `INIT_LOCK` serializes `open_once` so concurrent CEF requests cannot open duplicate SQLite handles or leak sync threads; `warm_restored_session` runs off the init lock on its own thread. Startup failures latch in `InitState::failed` (returning HTTP 503 `{ startupFailed: true }` until restart or explicit `recreate_library`) so repeated requests do not block the UI thread or rerun failing migrations. `SQLITE_BUSY`/`SQLITE_LOCKED` map to `library_refusal() == None` so `Recreate library` is never offered while another process holds the database open.
- **Single-writer preferences and durable roots.** `AppSettings::load_at_startup` is restricted to `main` and pre-service `locator`; runtime code uses `PreferencesService`, which holds the authoritative in-process snapshot so PATCHes never clobber CEF window geometry. `AccountConfigurationService::mutate` deliberately holds its mutex across `save_document` so read-modify-write and atomic file replacement stay serialized. Device `settings.json` zeroes per-account `appearance` before saving; `reader.json` lives with durable account preferences (not rebuildable cache) because Jellyfin keeps only coarse book positions. Environment data roots (`XDG_*`, `LOCALAPPDATA`, `APPDATA`, `HOME`) are ignored unless absolute so launching from another working directory never opens an empty `library.db`.

## Rejected alternatives

- Runtime convention-only account checks without `Scoped<T>` / `&Live`: easy to miss in new handlers, letting stale account responses leak into cache or UI.
- Re-reading `settings.json` on each API PATCH: races with CEF window-geometry saves and overwrites user state.
- Retrying failed `Services::open` on every API request: blocks the UI thread under `INIT_LOCK` and repeatedly attempts failing migrations.

## Consequences

- `Live` guards typed `Library` writers; `pub(crate)` raw SQL helpers (`with_connection`, `with_transaction`) remain outside `&Live` and are restricted to collection snapshot and identity-map writers.
- `PreferencesService` allows saving an unconfigured player path (validating executables at playback start) and checks `mark_watched_next` shortcut collisions only when `effective_backend() == Libmpv`.
- Collection failure recording reads existing `RefreshState` first and aborts logging on read error rather than overwriting `last_success`; `"movie-franchises"` avoids colliding with hex profile IDs in `collection_refresh_state`.
