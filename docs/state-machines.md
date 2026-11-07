# State machines audit

Audited at `772a6ff` (2026-11-07). Scope: the native app (`src/`) and the embedded UI (`ui/`). The Companion (`plugin/`, `src/companion/`, `src/seerr/`, the Seerr UI) is out of scope.

This audit came from reading the code only. Nothing was run against a real mpv, CEF or Jellyfin. Each finding is labelled with how far it was established:

- **Confirmed:** a reachable sequence was traced end to end in the code.
- **Confirmed (code):** the code path is traced, but one step depends on how an outside runtime behaves (Chromium, CEF, mpv, Jellyfin). That step is named.
- **Plausible:** depends on timing or an assumption the audit could not close. The open point is named.

Line numbers refer to `772a6ff`.

## Contents

1. [Prioritized bugs](#1-prioritized-bugs)
2. [Session and accounts](#2-session-and-accounts)
3. [Playback](#3-playback)
4. [Library sync and collections](#4-library-sync-and-collections)
5. [Shell, window and bridge](#5-shell-window-and-bridge)
6. [UI drafts and editors](#6-ui-drafts-and-editors)
7. [Checked and rejected](#7-checked-and-rejected)

---

## 1. Prioritized bugs

Ranked by user impact, then by how easily a user reaches the bug. Severities:

- **S1:** the app becomes unusable, or user data or account data is lost or wrong.
- **S2:** wrong watched or resume state, stuck UI, or persistent wrong behavior.
- **S3:** narrow window or minor effect.
- **S4:** cosmetic.

| ID | Sev | Status | Summary | Detail |
|---|---|---|---|---|
| P1 | S1 | Confirmed | Mark watched with no next item leaves mpv playing with no way to stop it from the app; plays started while that file is still loaded go unreported | [§3 PB-1](#pb-1-mark-watched-with-no-next-item-leaves-a-zombie-player) |
| P2 | S1 | Confirmed (code) | If services fail to start, the window-ready fallback is blocked by the app's own CSP and the window stays hidden forever | [§5 SH-1](#sh-1-window-ready-fallback-is-blocked-by-the-apps-own-csp) |
| P3 | S1 | Confirmed | A failed "Delete local account data" leaves a tombstone; after the user signs back in, the next launch deletes the account again | [§2 SE-1](#se-1-failed-account-deletion-is-replayed-after-the-user-signs-back-in) |
| P4 | S2 | Confirmed (code) | Signing out during playback revokes the token the reporter uses; final position and watched state are lost | [§3 PB-2](#pb-2-sign-out-during-playback-loses-the-final-report) |
| P5 | S2 | Confirmed (code) | Pressing mark watched twice reports the loading episode stopped at full runtime, without it playing | [§3 PB-6](#pb-6-double-mark-watched-marks-the-loading-episode-watched) |
| P6 | S2 | Confirmed | A second Play during a pending load is dropped, but the API answers `started: true` | [§3 PB-5](#pb-5-play-during-a-pending-load-is-dropped-but-reported-as-started) |
| P7 | S2 | Confirmed (code) | Context menu → Settings with an unsaved draft, then "Discard and leave", quits the app | [§5 SH-2](#sh-2-a-navigation-beforeunload-is-treated-as-a-window-close) |
| P8 | S2 | Confirmed | Refreshing a Letterboxd profile brings back a profile removed or disabled while the refresh ran | [§4 LS-1](#ls-1-letterboxd-refresh-resurrects-a-removed-or-disabled-profile) |
| P9 | S2 | Confirmed | A collection edit still saving brings back a collection deleted meanwhile | [§6 UI-2](#ui-2-in-flight-collection-edit-resurrects-a-deleted-collection) |
| P10 | S2 | Confirmed | Saving the Collections shelf pins the automatic collection mode, whatever was edited | [§6 UI-1](#ui-1-collections-save-pins-the-automatic-mode) |
| P11 | S2 | Confirmed | A failing franchise refresh loops with no backoff while the Movie Franchises page is open | [§4 LS-2](#ls-2-franchise-refresh-loop-with-no-backoff) |
| P12 | S2 | Confirmed | The daily re-download turns off ownership and collection work until it completes | [§4 LS-3](#ls-3-daily-re-download-disables-ownership) |
| P13 | S2 | Confirmed | Sign out, or delete account data, from Settings drops unsaved drafts with no prompt | [§6 UI-3](#ui-3-sign-out-drops-unsaved-drafts) |
| P14 | S2 | Confirmed | A collection created during an unsaved reorder vanishes; page Save then fails partway | [§6 UI-4](#ui-4-reorder-draft-hides-a-new-collection-and-save-fails-partway) |
| P15 | S2 | Confirmed | Wizard Preview overwrites edits typed while it ran | [§6 UI-5](#ui-5-wizard-preview-overwrites-concurrent-edits) |
| P16 | S2 | Confirmed (code) | No `on_render_process_terminated`: a renderer crash leaves a blank window, or a hidden one during startup | [§5 SH-3](#sh-3-renderer-crash-and-render-errors-are-not-recovered) |
| P17 | S2 | Plausible | The identity sweep can delete titles that still exist when the server changes during the walk | [§4 LS-4](#ls-4-identity-sweep-can-delete-live-titles) |
| P18 | S2 | Plausible | A late sign-in-page server probe repoints a signed-in session at another host and sends the token there | [§2 SE-2](#se-2-a-late-server-probe-repoints-the-live-session) |
| P19 | S2 | Plausible | libmpv runtime reset (IPC timeout) destroys the host window and the app closes | [§3 PB-7](#pb-7-libmpv-runtime-reset-closes-the-app) |
| P20 | S3 | Confirmed (code) | In Prompt mode (the default), resuming inside an intro skips the rest of the intro once | [§3 PB-3](#pb-3-non-user-seeks-count-as-seek-to-skip) |
| P21 | S3 | Confirmed | Stop is ignored while the next episode loads (until `file-loaded`, at worst 60 s) | [§3 PB-4](#pb-4-stop-is-ignored-while-the-next-item-loads) |
| P22 | S3 | Plausible | A render-time exception before the cover lifts would leave the window hidden; no concrete trigger traced | [§5 SH-3](#sh-3-renderer-crash-and-render-errors-are-not-recovered) |
| P23 | S3 | Confirmed | Home settings Save keeps failing with 409 after a sync changes the shelf elements | [§6 UI-6](#ui-6-smaller-ui-findings) |
| P24 | S3 | Confirmed | Manual-cadence collection detail pages stay on "Updating results" after their cache is wiped | [§4 LS-5](#ls-5-smaller-sync-and-collections-findings) |
| P25 | S3 | Plausible | Quitting during a sync can record a catalog failure, so collections are off at the next launch | [§4 LS-5](#ls-5-smaller-sync-and-collections-findings) |
| P26 | S3 | Confirmed | An expired Quick Connect code keeps polling every 2 s and the Start button never comes back | [§2 SE-3](#se-3-smaller-session-findings) |
| P27 | S3 | Confirmed | Restricted policy is checked once at launch; a failed check hides restricted-only content for the run | [§2 SE-3](#se-3-smaller-session-findings) |
| P28 | S2–S4 | Mixed | Remaining rows of the smaller-findings tables (SE-3, PB-8, LS-5, SH-4, UI-6); the S2 ones are Plausible: PB-8's `end-file` races and external `keep-open`, SH-4's second launch | §2–§6, "smaller findings" |

**Recommended order:** P1, P2 and P3 first. Each is a small change and stops the app from becoming unusable or losing account data. Then:

- **Playback:** P4–P6. P5, P6 and the Plausible `end-file` races in PB-8 share a root cause: the controller does not know which mpv playlist entry an `end-file` or `file-loaded` belongs to, and it abandons loads without stopping mpv (also P1). One change covers them: correlate on `playlist_entry_id`, and send `stop` whenever a load or playback is dropped without a replacement. P4 (stop and flush before logout) and P20 (startup seeks are not user seeks) are separate small fixes.
- **Shell:** P7.
- **Resurrections:** P8 and P9. Each needs the write re-checked against the current record under its lock.

---

## 2. Session and accounts

### 2.1 `SessionState` (`src/jellyfin/session.rs:12-25`)

Fields: `server_url`, `server_name`, `server_id`, `user_id`, `user_name`, `device_id`, `token`, `restricted`, `expired`, `generation`, `deleting`. Lock order: `operation_gate`, then `state`, then preferences, then the accounts document.

| State | Fields |
|---|---|
| Anonymous | `token=None`, `user_id=None`. `server_url` may be left over (`clear_local` keeps it, 497-514) |
| Live | `token`, `user_id`, `server_id`, `server_url` set; `!expired && !deleting` |
| Live, policy unverified | Live with `restricted=true` after restore (180-182) |
| Expired | Live fields with `expired=true` |
| Deleting | `deleting=true`, account fields kept |

**Invariants:**

- `scope()` is minted only from Live (`session.rs:259-282`).
- `confirm` and `Live::open` succeed only when generation and account match and the session is `!expired && !deleting` (`is_current`, 604-609).
- A 401 counts only for the current scope, or for the scope whose own 401 just expired the session (generation + 1). Anything older answers `AccountChanged` (`owns_failure`, 613-625).
- **Not enforced:** `server_url` is the host that issued `token` (see SE-2).

**Representable but meant to be impossible:**

- `token` and `user_id` without `server_id`. `status().authenticated` is true (578-592) but `scope()` fails with 409 (only a legacy or hand-edited row gets here).
- `server_url` pointing at a server other than the token's issuer (SE-2).

| Event | From | To | Guard | Location |
|---|---|---|---|---|
| restore at launch | none | Live (gen 1) or Anonymous (gen 0) | stored row complete | `session.rs:169-193` |
| `connect(url)` (sign-in page probe) | any | same, but `server_url` and `server_name` overwritten | **none**: no gate, no auth check, no generation bump | `session.rs:349-364` |
| login or Quick Connect → `commit_credentials` | any | Live, gen+1, `expired=deleting=false` | gate; catalog cleared when cache owner differs | `session.rs:414-484` |
| `scoped_error(Unauthorized)` | Live, same scope | Expired, gen+1, `SessionExpired` event | `owns_failure && !expired` | `session.rs:549-576` |
| `logout` / `clear_local` | any | Anonymous, gen+1 | gate; `library.clear_session` succeeds | `session.rs:486-514` |
| `begin_account_deletion` | Live or Expired, same account | Deleting, gen+1 | gate; account matches | `session.rs:516-530` |
| deletion completes | Deleting | Anonymous | account still current | `services.rs:435-444` |
| **deletion fails** | Deleting | **Deleting (no rollback, no event)** | — | `services.rs:406-407` |
| `refresh_user_policy` | Live, unverified | Live | `commit_if_current`; **called once, at launch** | `session.rs:229-257`, `services.rs:292-299` |

### 2.2 Stored credentials and schema (`src/library/session_store.rs`, `src/library/db.rs`)

- **States:** `credentials` row absent, full, or signed out (logout keeps server and device identity and clears user and token, `session_store.rs:89-92`); `meta` cache owner set or absent (it is in `CATALOG_TABLES`, `db.rs:400-409`); `user_version` below, equal to, or above `SCHEMA_VERSION`.
- **Invariant:** the catalog never belongs to an account other than the signed-in one. This is enforced at `session.rs:445-458`.
- **Gap:** `migrate` (`db.rs:154-197`) saves the session, drops every table, creates the schema, restores the session and sets `user_version` as separate autocommits. A crash in between loses the sign-in and the device id. For a v13 database it also makes `Library::open` fail at every launch afterwards, because `saved_legacy_account_data` reads tables that are gone (`db.rs:182-184`). Low priority while nobody runs released builds, but the fix is a single transaction.

### 2.3 Active preferences account (`src/preferences/service.rs:123-193`)

- **State:** `active_account: Option<AccountKey>`, plus appearance loaded for it.
- **Invariant:** while Live, `active_account == session.account_key()`. **Not enforced:** activation is a separate step after `commit_credentials` (`api/auth.rs:58-64`, 99-100, 146-161). If activation fails, or two sign-ins interleave, `patch_appearance` passes `commit_if_current` for the new account and writes under the old `active_account` (`service.rs:316-318`). Plausible; it needs an `accounts.json` write failure or concurrent sign-ins.

### 2.4 Account deletion journal

The steps run in this order (`services.rs:396-450`): Idle → journal entry (`deletions.rs:78-90`) → tombstone (Deleting) → remove from accounts, collections, playback preferences, snapshots and artwork → `clear_local(true)` and activate None → `finish`. Pending entries are resumed at every launch (`services.rs:316-328`).

**Invariant:** a pending journal entry means the account is not usable. Violated by SE-1.

### 2.5 UI sign-in and app gate

**App gate** (`App.tsx:42-69`): status pending → blank; catalog loading → blank; `BootstrapRetry`; `SignIn` unless authenticated or on Settings; otherwise the shell.

**What refetches status:** the `jellyfin-session-expired` event, mutation errors (`queries.ts:51-60`), and successful login, logout, Quick Connect or deletion. A **query** failing with 401 does not refetch status.

**Quick Connect** (`SignIn.tsx`, `queries.ts:619-631`) moves from idle to start (pending, then a code is shown), then polls every 2 s until authenticated. Polling decides from `data?.authenticated`, so it keeps going after an error (SE-3).

### SE-1. Failed account deletion is replayed after the user signs back in

**S1 · Confirmed** once a deletion step fails before `finish` removes the journal entry. Any of those steps can fail on an IO error: the `accounts.json` or `collections.json` write, the playback file or its backup scrub, an artwork file, `clear_local`, or SQLite busy. (A failure inside `finish` after the entry is removed, `deletions.rs:92-99`, does not leave a pending entry.)

1. `DELETE /api/collections/local-account` journals the deletion, then tombstones the session: `deleting=true`, gen+1 (`services.rs:403-407`).
2. A step fails, for example `accounts.remove_account` (`services.rs:414-417`). The handler answers 500. Nothing clears `deleting`, and no shell event is sent.
3. The UI's `onError` only shows a toast (`ApplicationSettings.tsx:66`) and does not refetch status. Every query now gets 401 because `scope()` refuses `deleting` (`session.rs:265-267`). Queries don't retry and don't refetch status, so the shell sits on dead controls until a mutation fails or the status pulse runs (it polls only while sync is active, `queries.ts:91-100`).
4. The user signs in again as the same account. `commit_credentials` sets `deleting=false` (`session.rs:480`). The journal entry stays.
5. At the next launch, `resume_pending_deletions` deletes the account again, along with any settings the user saved meanwhile, then signs the user out (`services.rs:316-328`, 435-444).

**Fix:** have `commit_credentials` (or the login handler) finish or drop a pending deletion for the account it signs in, before accepting. On failure, send `SessionExpired` (or have the delete `onError` invalidate `queryKeys.status`) so the UI leaves the dead shell.

### SE-2. A late server probe repoints the live session

**S2 · Plausible.** It needs the probe for the first server to finish after a login to a different server.

1. The sign-in page probes the saved URL X with `POST /api/auth/connect` (`SignIn.tsx:42-45`). X is slow; `public_system_info` retries within a 120 s budget (`jellyfin/api/client.rs:11-14`).
2. The user types server Y and signs in. `commit_credentials` sets `server_url=Y` and the token for Y.
3. The page unmounts and the probe fetch is aborted, but `auth_connect` ignores `request.cancelled` (`api/auth.rs:42-51`). It then runs `state.server_url = Some(X)` (`session.rs:354-357`) with no gate and no auth check.
4. `scope()` now builds `JellyfinClient::new(X, …, token for Y)` (`session.rs:268-279`). Every call sends Y's token to X. X answers 401 and the session expires. A restart recovers, because the database still holds Y.

**Fix:** make `connect` read-only. Nothing reads the `server_name` it stores; `SignIn.tsx:171` uses the probe's own answer.

### SE-3. Smaller session findings

| Finding | Status | Evidence | Fix |
|---|---|---|---|
| An expired Quick Connect code keeps polling every 2 s; Start stays hidden until the server field changes | Confirmed | `queries.ts:631` (`refetchInterval` reads stale `data`), `SignIn.tsx:101-105`, 116-137 | Stop on error; reset the mutation on 404 |
| The restricted policy is checked once at launch; if that fails, collections and Home "Missing" stay hidden all run | Confirmed | `services.rs:292-299`, `session.rs:237-241`, `session.rs:182` | Retry after the first successful scoped call |
| Home settings Save resolves Home from `session.account_key()` before taking its scope | Plausible | `home_settings.rs:19-22`, 331-341 | Take the scope first and resolve Home for `scope.account()` |
| A Quick Connect poll abandoned in the UI still signs in natively | Plausible | `quick_connect_poll` ignores `cancelled` | Check `request.cancelled` before `accept` |
| A delayed duplicate logout clears a session established after the first logout completed: Sign out stays enabled while pending (`AppSidebar.tsx:243`), and `logout` calls the server outside the gate, then `clear_local` with no generation check. It needs the second request's server call to outlast the first logout plus a full sign-in | Plausible | `session.rs:486-511` | Re-check the captured generation under the gate before clearing |
| `status().authenticated` and `scope()` use different predicates | Plausible (legacy row) | `session.rs:581-584` vs 268-275 | Use one predicate |
| Appearance writes go to `active_account`, not to the scope's account | Plausible | §2.3 | Pass `scope.account()` into `patch_appearance` |
| Schema recreation is not atomic | Plausible (crash) | §2.2 | One transaction |

---

## 3. Playback

### 3.1 Controller phase (`src/players/mpv/controller.rs`)

Fields: `phase` (Idle / Loading(pending) / Playing(active), 215-219), `mpv_playback_active`, `playback_identity`, `startup_seek`, `replacement_end_file_pending`, `next_playback_handoff_until`, `recent_loads`.

| Combination | Meaning |
|---|---|
| Idle, `active=false` | Nothing tracked. **mpv may still be playing** (PB-1) |
| Idle, `active=true` | Activated without a reporter (no token), `playback_transition.rs:233-239` |
| Loading, `active=true` | `loadfile` accepted, waiting for `file-loaded` |
| Playing, `active=true` | Normal playback |
| Playing, `active=false` | Not reachable: every path that clears `active` takes the active playback first |

**Invariant the code relies on but does not enforce:** whenever the controller leaves Loading or Playing without a replacement load, mpv is stopped. `finish_active(WatchedNext)` and the 60 s pending timeout break it.

| Event | From | To | Guard | Location |
|---|---|---|---|---|
| Load | Idle / Playing | Loading. Playing → old reporter `report_stopped`, no Stopped event | not a duplicate; **no pending load**; `ensure_mpv` and `loadfile` succeed | `controller.rs:718-803` |
| Load | Loading | Loading (**new load dropped silently**) | — | `controller.rs:737-745` |
| `loadfile` rejected while replacing | Playing | Idle + `reset_mpv` | — | `controller.rs:804-806`, 817-834 |
| `file-loaded` | Loading | Playing | reporter present | `playback_transition.rs:197-251` |
| `end-file` stop/redirect | Loading | Loading (ignored) | `replacement_end_file_pending`, or handoff deadline open | `playback_transition.rs:382-396` |
| `end-file` other | Loading / Playing | Idle, Stopped; EOF arms the handoff | — | `playback_transition.rs:289-367` |
| MarkWatchedAndPlayNext | Loading / Playing / Idle+active | Idle, handoff armed; **mpv not stopped** | not (Idle and `!active`) | `playback_transition.rs:253-287` |
| pending timeout (60 s) | Loading | Idle (Error); **mpv not stopped** | — | `session.rs:579-591` |
| runtime died / IPC lost | any | Idle, `reset_mpv`, restart | `session_id` matches | `session.rs:386-402`, `events.rs:45-54`, 72-76 |
| user Stop | Loading with handoff armed | **dropped** | handoff deadline open | `controller.rs:611-619` |

### 3.2 Startup seek (resume hold)

| Event | From | To | Location |
|---|---|---|---|
| `file-loaded`, start > 0 | None | Some(due = now + 500 ms) | `controller.rs:645-671` |
| due; seek Ok or Rejected | Some | Some(due = now + 1 s); re-sent every second | `controller.rs:692-709` |
| `time-pos` ≥ target − 3 s | Some | None; external subtitle loads | `segment_skip.rs:466-495` |
| `time-pos` below that | Some | Some; the reported position is held | `segment_skip.rs:475-483` |
| finish / reset / load | any | None | `playback_transition.rs:297`, `session.rs:627`, `controller.rs:761` |

The hold also undoes a user seek backwards during the first second or two (S4, Confirmed).

### 3.3 Segment skip

- A `seek` event sets `seek_started_at` only when the position is inside a Prompt segment (`segment_skip.rs:276-292`).
- `seeking=true` sets it **always** (294-299).
- The next forward `time-pos` while it is set counts as "Seek to Skip" (304-335).
- `triggered` is set only after the skip seek succeeds (404-458).
- Segments arriving late are dropped unless they match the current `playback_id` (16-56, 72-85).

### 3.4 Reporter, socket, overlay, autoplay

- **Reporter** (`playback_reporter.rs`):
  - One thread with a FIFO queue. Queued progress for the same item is replaced by newer progress or by stop (181-199).
  - Auth headers are fixed at launch (240).
  - Progress is sent only while Playing (`session.rs:366-384`).
- **Socket** (`socket.rs`):
  - States: Unauthenticated, Connecting, Listening, Backoff, Stopped.
  - The backoff doubles to 10 min and resets only after a connection lasting ≥ 60 s (197-212). `SessionChanged` still waits out the backoff (S3, Confirmed).
- **Overlay:** binds once, to the first libmpv runtime's window (`shell/cef/runtime.rs:145-167`). If that window goes away, the app closes (Linux `linux.rs:355-366`, Windows `windows.rs:290-305`).
- **UI autoplay** (`ui/src/lib/playback-events.ts:40-101`):
  - State: `completed`, `continuing`, `stoppedItem`, timer and toast, `promptGeneration`.
  - Stopped with `eof` or `watched-next` → `playNext`, or a countdown.
  - If `playNext` returns `started:false`, the UI does nothing (79).

### PB-1. Mark watched with no next item leaves a zombie player

**S1 · Confirmed.**

1. A movie, or a series finale, is playing.
2. The user presses the mark-watched key (PlayerBar, `PlayerBar.tsx:303-305`, or the mpv binding, `input.rs`).
3. `finish_active(WatchedNext)` reports the item stopped and sets `mpv_playback_active=false` (`playback_transition.rs:285`, 339-363). mpv is not stopped and keeps playing.
4. The UI calls `playNext`, which returns `NOT_STARTED` (`api/playback.rs:132`). The UI returns (`playback-events.ts:79`).
5. The snapshot says `active=false`, so the PlayerBar and its key listener unmount (`PlayerBar.tsx:673`). With built-in libmpv, mpv's own bindings are off (`input-default-bindings=no`, `input-vo-keyboard=no`, `runtime.rs:235-236`), so quitting the app is the only way to stop it.
6. mpv runs with `idle=yes` (`runtime.rs:239`). When the orphaned file ends, its `end-file` meets Idle with `!active` and nothing happens, and later plays are normal. But if the user plays item C while it is still loaded and after the 60 s handoff has expired:
   - `replacement_end_file_pending` is false, because `mpv_playback_active` was false (`controller.rs:763`).
   - The old file's `end-file(stop)` therefore takes C's pending load and emits Stopped (`playback_transition.rs:309-329`).
   - C's `file-loaded` hits "without pending playback" (198-200). C plays with no start, progress or stop reports and no PlayerBar.

The same happens when `playNext` fails, and after the 60 s pending timeout if mpv later finishes loading.

**Fix:** when the controller drops a load or playback with no replacement, send mpv `stop`. Concretely, in `mark_watched_and_play_next` when the handoff expires unused, and in the pending timeout. A simpler fix is for the UI to send `stop` when `playNext` returns `started:false` or fails, but that does not cover the mpv key binding when the UI is not listening.

### PB-2. Sign-out during playback loses the final report

**S2 · Confirmed (code).** That Jellyfin revokes the token on `/Sessions/Logout` is server behavior.

- `auth_logout` → `session.logout` revokes the token and clears local state (`api/auth.rs:114-131`, `session.rs:486-494`). Nothing stops playback.
- The reporter keeps its launch headers (`playback_reporter.rs:240`). Every later progress report and the final Stopped get a 401, which is only logged (226-232).
- So the resume position and watched state from the rest of the session are lost. (Account deletion clears local state with `clear_local` and does not revoke the token, so it does not have this gap.)

**Fix:** in logout, stop playback and `flush_playstate_reports` with a bounded timeout before revoking.

### PB-3. Non-user seeks count as "Seek to Skip"

**S3 · Confirmed (code)** for resume: the rest of the intro is skipped once. This assumes mpv's exact seek lands at or after the target. Prompt is the default for every segment type (`preferences/model.rs:297-300`).

1. Resume inside an intro. The startup seek's `seek` event sets `seek_started_at` to the held position, because it is inside a Prompt segment (`segment_skip.rs:276-291`).
2. The first forward `time-pos` sample after the hold releases is accepted as a user skip (312-334). The intro is skipped.

Starting from 0 with a segment at 0 is **Plausible** by the same route: `seeking=true` sets the marker unconditionally (294-298), assuming mpv reports `seeking=yes` before the first `playback-restart`.

**Fix:** don't set `seek_started_at` while `startup_seek` is Some or before the first `playback-restart`.

### PB-4. Stop is ignored while the next item loads

**S3 · Confirmed.** Stop works again once `file-loaded` clears the handoff (`playback_transition.rs:240`); the 60 s is the worst case, when it never arrives.

1. Episode A ends: `finish_active(Eof)` arms the 60 s handoff (`playback_transition.rs:364-366`).
2. Autoplay (or any play within 60 s) puts B in Loading. The PlayerBar shows, because `active` is true.
3. Stop from the PlayerBar or from remote control (`remote.rs:252`) returns early (`controller.rs:611-619`). B starts playing anyway.

The old file's stale `end-file` is already filtered separately (`playback_transition.rs:382-396`), so this second suppression only ever swallows explicit user Stops.

**Fix:** remove the suppression in `control`. Instead, treat Stop during a pending load as "cancel the load": send `stop` and finish the pending load as Stop.

### PB-5. Play during a pending load is dropped but reported as started

**S2 · Confirmed.** Play A, then Play B (or a quality change, or a remote Play) before A's `file-loaded`.

- B is dropped (`controller.rs:737-745`) with no Failed event.
- `/api/play` has already answered `started:true` for B (`api/playback.rs:187-193`, `jellyfin/play.rs:104-109`).
- A plays.

**Fix:** let the new load replace the pending one (take the pending load, set `replacement_end_file_pending`), or emit Failed.

### PB-6. Double mark-watched marks the loading episode watched

**S2 · Confirmed (code).** That Jellyfin marks an item played from a Stopped report at full runtime with no Start is server behavior.

1. The first press marks A watched, and the UI starts loading B.
2. A second press during Loading passes the idle check (`playback_transition.rs:254`), sets the position to B's runtime (259-272), and `finish_active` reports B stopped at full runtime (320-327).
3. B is marked watched without playing, and autoplay jumps to C.

**Fix:** ignore MarkWatchedAndPlayNext while the phase is Loading.

### PB-7. libmpv runtime reset closes the app

**S2 · Plausible.** It needs an IPC transport failure, for example a reply slower than 5 s, which poisons the writer (`ipc.rs:209-226`).

1. `ensure_mpv` sees no live IPC worker and runs `finish_active` + `reset_mpv` (`session.rs:212-219`, 637-640).
2. With built-in libmpv this destroys the host window (`linux_window.rs:182-185`).
3. The overlay is bound only once, so it sees its parent gone and closes the app (Linux `linux.rs:359-362`, Windows `windows.rs:296-299`).

Related, also **Plausible** and Linux only: `present_regions` blocks the CEF UI thread up to 5 s on the controller thread (`coordinator.rs:67-75`). The controller can be inside `sub-add` (30 s) or an IPC connect (15 s). A timeout closes the app (`linux.rs:637-640`).

**Fix:** for libmpv, replace only the IPC worker, or rebind the overlay to the new window. Treat a presentation timeout as a skipped frame.

### PB-8. Smaller playback findings

| Finding | Sev | Status | Evidence |
|---|---|---|---|
| Stop sent right after a replacement load: mpv may emit one `end-file` that is consumed as "the replaced file's"; the controller sits in Loading for 60 s, ignores Plays, then reports Failed | S2 | Plausible (mpv merging events) | `controller.rs:797`, `playback_transition.rs:391-394`, `session.rs:579-590` |
| The old file's `end-file(eof)` during a replacement completes the new item and marks it watched | S2 | Plausible (ms window) | `playback_transition.rs:386-389`, 320-337 |
| External mpv with `keep-open=yes` in the user's `mpv.conf` never reaches EOF; `eof-reached` is stored but never read | S2 | Plausible (user config) | `external.rs:28-47`, `events.rs:257-268` |
| Replacing an active item reports it stopped to Jellyfin but sends no Stopped event to the UI | S4 | Confirmed | `controller.rs:764-771` |
| A remote command that passed its scope check, or that the socket read just before noticing the scope changed, still runs after sign-out: a queued load (`play.rs:90-108`, `controller.rs:390-400`, 537-543) or Pause/Stop/Seek (`socket.rs:306-318`, `remote.rs:247-259`). The socket checks the scope before a blocking read with a 5 s timeout (`socket.rs:47`), so the window is up to 5 s | S3 | Plausible (scope change during a socket read or while a load is queued) | — |
| The PlayerBar can briefly reappear after Stop (a stale `playerState` GET lands after the Stopped event) | S4 | Plausible | `queries.ts:568-570` |
| Windows: reinstalling mpv fails while the warmed `mpv.exe` runs from the install directory | S3 | Plausible | `player_setup.rs:110-115` |

---

## 4. Library sync and collections

### 4.1 Sync worker (`src/library/sync/worker.rs`, `src/library/sync.rs`)

State: `running: AtomicBool`; `Flags{requested, stopped, window_ready}`; the loop locals `startup_hold`, `trigger`, `backoff`, `normal_deadline`.

| Event | From | To | Guard | Location |
|---|---|---|---|---|
| thread start | — | Hold (15 s) | signed in and `catalog.ready` | `worker.rs:40-41` |
| signed out | any | IdleUnauth (wait 30 s) | `!is_authenticated` | `worker.rs:43-52` |
| window ready | Hold | Hold, deadline ≤ now + 3 s | — | `worker.rs:54-67` |
| `request()` | any wait | Requested; coalesced if a cycle runs | flag consumed by `wait()` | `sync.rs:308-313`, `worker.rs:161-178` |
| cycle Ok | Running | Waiting (10 min + jitter), backoff reset | — | `worker.rs:78-92` |
| cycle Cancelled (account changed) | Running | Waiting (30 s) | — | `worker.rs:94-97` |
| cycle Unauthorized | Running | Waiting, then IdleUnauth | session expired | `worker.rs:98-101` |
| cycle error | Running | Waiting (backoff); failure tagged with generation | 5→60 s if `BOOTSTRAP_DONE≠1`, else 20→30 min | `worker.rs:102-114`, 137-144 |
| `stop()` | any | Exit | — | `sync.rs:330-335` |

`SyncPhase` (Catalog, Reconciling, Complete) is set to Catalog or Reconciling during a cycle (`cycle.rs:47-61`) and never reset. A new cycle shows the previous cycle's phase until `cycle.rs:50` runs (S4, Confirmed).

### 4.2 Persisted sync cursor (`meta` table)

| State | `BOOTSTRAP_DONE` | `LAST_BOOTSTRAP` | Meaning |
|---|---|---|---|
| Fresh | absent | absent | First download |
| Filling | 0 or absent | absent | Resumable first download (`OFFSET`) |
| Complete | 1 | t | Normal |
| Refilling | 0 | old t | Daily re-download (`cycle.rs:44-46`, 130-147, 782-789) |

| Event | From | To | Location |
|---|---|---|---|
| page committed | Fresh / Filling / Refilling | `OFFSET += n`, `READY=1` | `cycle.rs:393-425` |
| empty or short page | Filling / Refilling | Complete | `cycle.rs:324-334`, 427-441 |
| 24 h since `LAST_BOOTSTRAP` | Complete | Refilling | `cycle.rs:782-789` |
| account switch | any | Fresh (`meta` wiped) | `db.rs:400-409` |
| cycle error / success | — | `LATEST_FAILURE` = 1 / 0 | `cycle.rs:90-92`, 149-174 |

**Invariants:**

- Watermarks advance only after every page of a pass commits (`cycle.rs:374-380`, 551-558).
- `ownership_available = BOOTSTRAP_DONE=1 && LATEST_FAILURE≠1` (`sync.rs:107-114`).
- Every catalog write goes through `commit` → `commit_if_current` behind the operation gate (`sync.rs:417-423`).

### 4.3 Identity sweep verdict (`cycle.rs:586-629`)

| Walk result | Verdict |
|---|---|
| Incomplete (error or page cap), or empty with a nonzero count | ForgetEmpty |
| **Anything seen** | **Delete everything not seen** |
| Empty with count 0 and no catalog library | Delete |
| Empty with count 0 | Observe, then Delete if confirmed 10 min – 2 h later |

### 4.4 Collections

- **Refresh state** per (account, profile): `{last_attempt, last_success, latest_failure, next_due, initialized}`. Snapshots are keyed by (account, profile, revision). Neither table is in `CATALOG_TABLES`; schema recreation wipes both.
- **`ResultsStatus`** (`api/collections/browse.rs:125-146`):

  | Snapshot for the current revision | `latest_failure` set, or profile invalid | Status |
  |---|---|---|
  | yes | — | Ready |
  | no | yes | ResultsUnavailable |
  | no | no | Updating |

- **Writers:**
  - User-initiated create, edit and refresh commit under `commit_if_current` (`profiles.rs:544-589`).
  - The scheduler's success path writes snapshot and state guarded only by `profile_is_current` (`scheduler.rs:343-374`). Plausible orphan rows if the profile or account is deleted in between.
- **Concurrency guards:**
  - Franchise refresh: one in flight per account (`FranchiseFlight`, `scheduler.rs:21-47`).
  - Profile refresh: **none.** The hourly loop, `request_run` and `request_after_library_sync` can refresh the same due profile at once (S3, Confirmed).
- **`CollectionMode`:** the effective mode is the stored `mode_selection`, or MediaFlick when TMDB is ready, profiles exist, or results are cached (`preferences/collections.rs:123-137`).

### LS-1. Letterboxd refresh resurrects a removed or disabled profile

**S2 · Confirmed.**

1. In Letterboxd settings, click "Refresh profile" on A. `verify` takes up to about 17 s.
2. Meanwhile, remove A (or turn it off) and Save. The fieldset is disabled only by the save, not by the refresh (`LetterboxdSettings.tsx:187`, 201).
3. The refresh finishes. It built its record from the copy read before the network call (`..existing`, `api/letterboxd.rs:184`, 199-206). `save_letterboxd_profile` finds no matching id or key and **pushes** A back (`accounts.rs:519-531`). A disable is reverted the same way.

`commit_if_current` checks the account, not the record.

**Fix:** under the gate, re-read the profile by id. Return 404 if it is gone, and update only `verification_status`, `display_name`, `canonical_url` and `last_checked_at`.

### LS-2. Franchise refresh loop with no backoff

**S2 · Confirmed.** Needs the Movie Franchises page open and the refresh failing (for example, a Companion without the franchise capability fails instantly).

1. `GET /collections/franchises` sees ownership and `!initialized`, so it calls `request_run` (`browse.rs:201-203`).
2. `refresh_franchises` fails, saves the failure and returns `true` (`scheduler.rs:160-169`). `initialized` stays false.
3. `refresh_due_work` emits `CollectionsChanged` (`scheduler.rs:81-83`). The UI invalidates the `collections` root (`library-events.ts:74-78`), which refetches franchises, which calls `request_run` again.
4. Each round also re-runs every failing due profile (more MDBList quota) and `hydrate_identity_map`.

The desktop sends every stale id in one request and requires every one back (`scheduler.rs:186-190`). Whether the Companion caps that request is for the Companion audit.

**Fix:** emit `CollectionsChanged` only when results changed. Skip `request_run` in `franchises()` when `last_attempt` is recent.

### LS-3. Daily re-download disables ownership

**S2 · Confirmed.**

- Once `LAST_BOOTSTRAP` is older than 24 h (usually at the first launch of the day), `restart_bootstrap` sets `BOOTSTRAP_DONE=0` (`cycle.rs:130-147`).
- `ownership_available` is then false until the whole re-download finishes. During that time:
  - the owned/missing split disappears;
  - Home collection rows are empty (`home.rs:212`);
  - the scheduler skips all work (`scheduler.rs:98-100`);
  - the sidebar shows "Loading library N of M".
- If a page keeps failing, this persists, with first-download backoff (5→60 s).

`first_catalog_download` already tells the two cases apart (`sync.rs:101-105`).

**Fix:** base `ownership_available` and `next_backoff` on `first_catalog_download`.

### LS-4. Identity sweep can delete live titles

**S2 · Plausible.** Needs a server-side deletion during the walk, or unstable ordering of ties.

1. `walk_identity` pages with `SortBy=DateCreated` only and `offset += page.len()` (`items.rs:107-108`, `cycle.rs:654-683`).
2. If the server deletes an item before the current offset between two pages, the next page skips one title.
3. The walk is still "Listed", so the verdict is Delete, and `retain_ids` removes the skipped title (`cycle.rs:596`, `catalog.rs:114-131`).
4. The incremental pass only fetches items newer than the watermark, so the title stays missing until the next daily re-download.

The membership walk has both guards this one lacks: a `DateCreated,SortName` tie-break (`items.rs:149-151`), and "fewer ids than counted means keep" (`sync/libraries.rs:153-162`).

**Fix:** add the same tie-break, and treat the walk as Incomplete when `seen.len()` is below the first page's `total_record_count`.

### LS-5. Smaller sync and collections findings

| Finding | Sev | Status | Evidence | Fix |
|---|---|---|---|---|
| Manual-cadence collection detail pages stay on "Updating results" after `library.db` is recreated; the detail retry button needs a failure or an overdue state. My Collections → "Check for updates" still recovers it (`Collections.tsx:236-248`) | S3 | Confirmed | `scheduler.rs:414-419`, `browse.rs:127-143`, `CollectionDetail.tsx:309-327` | Treat `!initialized` as due for every cadence |
| Quitting during a sync can record `LATEST_FAILURE=1`; collections are off at the next launch until the first cycle completes. `stop()` does not join the worker, so this needs the thread to reach `record_failure` before `process::exit` | S3 | Plausible (timing) | `cycle.rs:90-92`, 149-155; `sync.rs:410-411` | Skip `record_failure` for `Cancelled` |
| Due profiles can be refreshed by two runs at once | S3 | Confirmed | §4.4 | Per-profile in-flight set |
| Hitting the incremental page cap advances the watermark past items never fetched (needs > 20,000 new items) | S3 | Confirmed (rare) | `cycle.rs:512-558` | Don't commit the watermark on cap |
| "Because you watched" seed cached as None for the rest of the run when computed before the first page | S4 | Confirmed | `home.rs:162-176` | Don't cache None |
| Scheduler success path writes outside the gate (orphan rows) | S4 | Plausible | `scheduler.rs:343-374` | Wrap in `commit_if_current` |
| `SyncPhase` never reset (stale label at cycle start) | S4 | Confirmed | `cycle.rs:47-61` | Set the phase before `list_libraries` |

---

## 5. Shell, window and bridge

### 5.1 Main window reveal (`src/shell/cef/mod.rs:319-390`)

The window is created hidden (`runtime.rs:385-391`). State: `main_window`, `reveal_requested`, `revealed`.

| Event | From | To | Location |
|---|---|---|---|
| `on_window_created` | — | Created; or Revealed if a reveal was requested earlier | `mod.rs:338-346` |
| `/api/shell/window/ready` | Created | Revealed | `api/shell.rs:34`, `mod.rs:354-390` |
| `mediaflick-desktop://window-ready` beacon | Created | Revealed | `bridge.rs:50-52`, 69 |
| `data:` error page `on_load_end` | Created | Revealed | `handlers.rs:395-398` |

**Invariant:** the window is shown once, after the page reports ready. **There is no timeout, and no reveal on a renderer crash or a React failure.** SH-1 and SH-3 follow from that.

### 5.2 Close and quit

State: `force_close_requested` (only ever goes false → true), the `browsers` list, and the CEF close state.

| Event | From | To | Guard | Location |
|---|---|---|---|---|
| X / Alt+F4 | Open | Minimized | Close behavior is Minimize and not forced | `mod.rs:298-317` |
| X / Alt+F4 | Open | CloseRequested | otherwise | `mod.rs:314-316` |
| beforeunload cancelled by the page | **any unload** (close **or navigation**) | Open + `WindowCloseBlocked` broadcast | not forced | `handlers.rs:70-89` |
| "Discard and leave" | Open | `window.close()` | — | `SettingsDraftGuard.tsx:38-41` |
| update installer exit | any | ForceClosing | — | `bridge.rs:214-240` |
| last browser closed | Closing | stop playback, sync, socket; quit; `process::exit` | — | `handlers.rs:330-367`, `main.rs:105` |

### 5.3 Other shell machines

- **Services init** (`locator.rs`):
  - States: Uninit → Opening → Ready or Failed. Any later API call retries a Failed init (`api.rs:126-145`).
  - The shell and preference bridges and `attach_playback` subscribe only if services are Ready when the browser state is created (`mod.rs:402-404`, `events.rs:133-135`, 152-154).
  - A later successful init never subscribes them. Plausible: the shell bridge stays dead and player commands answer 503.
- **ShellBridge** (`services.rs:92-116`): Unsubscribed (events dropped) → Subscribed. Before the UI's `useLibraryMetadataBridge` mounts, only `libraries-changed` is buffered (`library-events.ts:34-42`). Other events, such as `collections-changed` from the scheduler, which starts at once (`services.rs:280-282`), are dropped (S3, Plausible).
- **Update** and **mpv install** use one-shot flags with explicit reset on error (`bridge.rs:242-330`, `events.rs:497-603`). No reachable bad state was found, except that a page reload forgets the install request id.
- **UI bootstrap** (`main.tsx`, `App.tsx`, `LoadingScreen.tsx:29-59`):
  - Sequence: PreBridge → Rendered → bridge mounted → CoverLeaving → CoverRemoved, which calls `windowReady()`.
  - `windowReady()` falls back to the image beacon if the call fails.
- **NavigationHistory** (`NavigationHistory.tsx:10-37`): `{entries, index}` is updated in a layout effect after a POP commits. A double click on BackLink before then goes back two entries (S4, Plausible).
- **Live search** (`use-live-search.ts`): the draft is keyed to its `source`. A keystroke typed while the router's navigation transition is pending is dropped when the transition commits (S4, Plausible).

### SH-1. Window-ready fallback is blocked by the app's own CSP

**S1 · Confirmed (code).** That Chromium refuses the image before any request goes out is runtime behavior, not yet seen in a console.

1. Services fail to start, for example `accounts.json` written by a newer build (`accounts.rs:690`) or `library.db` unopenable. Every API call answers 503 (`api.rs:138-143`).
2. SignIn renders and the cover lifts. `api.shell.windowReady()` fails, so the page inserts `<img src="mediaflick-desktop://window-ready">` (`LoadingScreen.tsx:45-55`).
3. Every app response carries `img-src 'self' data:` (`app_scheme.rs:298-302`). `'self'` is `mediaflick-desktop://app`, so the `window-ready` host does not match. The scheme is not registered CSP-bypassing (`runtime.rs:92-95`).
4. The image is refused, `reveal_main_window` never runs, and the hidden window stays hidden. On Windows, relaunching then only says "already running" (`instance.rs:67-91`).

`connect-src` and `frame-src` already allow `mediaflick-desktop:`, but `img-src` does not.

**Fix:** add `mediaflick-desktop:` to `img-src`, or send the beacon as a hidden iframe. Add a native reveal deadline as well; it also covers SH-3.

### SH-2. A navigation beforeunload is treated as a window close

**S2 · Confirmed (code).** The last step relies on CEF's close path: `window.close()` → `do_close` (returns 0, `handlers.rs:326-328`) → the window's `can_close`.

1. Edit any Settings shelf so it is dirty.
2. Right-click and choose "Settings". `open_settings_page` does a full `frame.load_url(...)` (`bridge.rs:203-212`), not a router push.
3. The page cancels beforeunload (`SettingsDraftGuard.tsx:24-26`). The native handler broadcasts `WindowCloseBlocked` for every unload; `is_reload` is only logged (`handlers.rs:77-87`).
4. The UI shows "Leave without saving?". "Discard and leave" calls `window.close()`, which quits the app with the default Close behavior (`ExitApp`, `preferences/model.rs:373-376`). With Minimize it minimizes the window instead (`mod.rs:303-312`), and the user is still not taken to Settings.

**Fix:** open Settings through an in-app navigation (a shell event the router handles). Broadcast `WindowCloseBlocked` only while a close is actually pending.

### SH-3. Renderer crash and render errors are not recovered

**S2 · Confirmed (code)** for the renderer crash: the handler is missing, and the crash itself is external. **S3 · Plausible** for the render exception: the missing error element is confirmed, but no concrete exception trigger was traced.

- **No renderer-crash handler.** `JellyfinRequestHandler` has no `on_render_process_terminated` (`handlers.rs:438-505`). A renderer crash before ready leaves the window hidden; after ready, it leaves it blank. `browsers` keeps the dead browser.
- **No root error element.** The root route has no `errorElement` (`main.tsx:19`). A render exception behind the loading cover replaces the whole tree, `LoadingScreen` included, so `windowReady` never runs.

**Fix:**

- Handle `on_render_process_terminated` by reloading the app URL or showing the error page.
- Give the root route an `errorElement` that still reports ready.
- Add a native reveal deadline.

### SH-4. Smaller shell findings

| Finding | Sev | Status | Evidence |
|---|---|---|---|
| Second launch on Linux/macOS: no single-instance guard, so a second process starts services and sync threads before CEF refuses it; with no `on_already_running_app_relaunch`, CEF's default opens a stray Chrome window in the first process, which then keeps the app alive after the main window closes | S2 | Plausible (CEF default) | `instance.rs:94-96`, `mod.rs:107-115`, `runtime.rs:116-217`, `handlers.rs:336-349` |
| `process::exit` skips the log guard's flush; the shutdown log lines are lost | S3 | Confirmed | `main.rs:58`, 61, 64, 75, 105 |
| Windows first launch race: two processes write different instance ids and both pass the mutex | S3 | Plausible | `instance.rs:17-27`, 71 |
| Window stays hidden up to the HTTP timeout when restarting with an unready catalog against a slow server | S3 | Plausible | `App.tsx:43`, 64; `client.rs:11` |
| Bridge threads may `post_task` during CEF `shutdown()` | S3 | Plausible | `events.rs:383-458`, `handlers.rs:365` |
| mpv install finishing after the Player shelf remounts saves the path natively but never invalidates `settings`; a later Save writes the old path back | S3 | Plausible | `PlayerSettings.tsx:103-112`, `events.rs:580-589` |

---

## 6. UI drafts and editors

### 6.1 Settings draft (`use-source-draft.ts`, `SettingsSaveBar.tsx`)

The draft holds `{source S0, value V, account A0}`. The query supplies the current source S and account A. Derived states: Loading, Pristine (V ≈ S), Dirty, Saving, Failed (still dirty).

| Event | From | To | Guard | Location |
|---|---|---|---|---|
| edit | Pristine / Dirty / Saving | Dirty, or Pristine if V = S | — | `use-source-draft.ts:26-39` |
| source changes | any | `mergeDraft`: top-level keys merged; **an array or object the user changed is kept whole** | — | `use-source-draft.ts:4-24` |
| account changes | any | V := S | account differs | `use-source-draft.ts:19-22` |
| Save | Dirty | Saving | button disabled while saving or clean | `SettingsSaveBar.tsx:25` |
| success | Saving | Pristine; keys edited during the save survive | `acceptSaved` | `use-source-draft.ts:40-44` |
| failure | Saving | Dirty | — | — |
| Discard / Reset | Dirty / any | Pristine / Dirty (defaults) | disabled while saving | `SettingsSaveBar.tsx:23-24` |

The **route guard** (`SettingsDraftGuard.tsx:8-60`) has a registry `id → saving`; the guard is dirty when the registry is non-empty. It hooks three things:

- `useBlocker` blocks router navigation.
- `beforeunload` cancels unloads.
- The guard is keyed by `accountKey`, so it **remounts and drops every draft** when the account key changes (`Settings.tsx:85`).

**Representable but meant to be impossible:**

- Dirty with nothing visible: `deletedProfileIds` lists an id that is already gone after a partial Save (UI-4).
- Saving with the editor unmounted: the wizard closed with Escape during a save (UI-2).

### 6.2 Collections page and wizard (`CollectionSettings.tsx`)

- **Page draft:** `{modeSelection, includeUnreleased, profileIds, deletedProfileIds}`. The baseline is built from two queries (662-667). Save is not atomic: N deletes, then the settings PATCH, then an optional reorder (670-683).
- **Wizard:** open (new or edit) × draft × previewing × uploading × save pending.
  - Preview replaces the draft with the copy taken at click time (412-428).
  - Escape or an overlay click closes the wizard even while saving (490).

### UI-1. Collections Save pins the automatic mode

**S2 · Confirmed.** The hidden automatic mode is lost and replaced by the mode shown at save time; a partial save is possible.

- The baseline uses `modeSelection ?? effectiveMode` (`CollectionSettings.tsx:663`), and every Save sends `modeSelection` (675-678).
- `set_mode` stores `Some(mode)` (`preferences/collections.rs:146`). The automatic mode exists only as `None`, and the Select cannot represent it.
- So toggling "Include unreleased", reordering or deleting a collection fixes the mode at whatever was effective then. If TMDB was not ready yet, the account stays on Jellyfin mode for good.

Plausible follow-on:

1. The user deletes the last profile while TMDB is not ready and no results are cached.
2. The deletes commit, then the PATCH with `mediaFlick` answers 409 (`profiles.rs:188-199`).
3. The shelf stays dirty, holding an invisible pending delete.

**Fix:** send only fields that differ from the saved draft, and skip the PATCH when neither changed.

### UI-2. In-flight collection edit resurrects a deleted collection

**S2 · Confirmed.** The timing is user-paced, because a result-affecting edit waits seconds on a provider refresh.

1. Edit collection X with a result-affecting change and click Save.
2. Press Escape while it saves (`CollectionSettings.tsx:490`).
3. Mark X deleted and Save the page. `deleteProfile(X)` commits.
4. The edit's commit calls `save_profile`, which upserts X back (`preferences/collections.rs:182-190`). The edit path checks that X exists only at entry (`profiles.rs:359-361`).

If another wizard was opened meanwhile, the first mutation's `onSuccess` calls `onOpenChange(false)` (`CollectionSettings.tsx:341`), which closes the new wizard and loses its edits.

**Fix:** make the edit commit update-only (fail if the profile is gone, checked under the `mutate` lock). Block closing the wizard while saving.

### UI-3. Sign out drops unsaved drafts

**S2 · Confirmed.** With a dirty shelf (device-level Player, Playback or Application included):

1. Sign out from the sidebar menu (`AppSidebar.tsx:243`), or delete local account data.
2. `accountKey` changes and the guard remounts (`Settings.tsx:85`). Every draft is gone without the "Leave without saving?" prompt; `useBlocker` only covers navigation.

**Fix:** consult the draft registry before logout and delete, or key only the account-scoped shelves.

### UI-4. Reorder draft hides a new collection and Save fails partway

**S2 · Confirmed.**

1. Move a collection in the list. The `profileIds` draft is now user-owned.
2. Add a collection from a template. It commits immediately, but `mergeDraft` keeps the user's array whole (`use-source-draft.ts:8`, 11), so the new collection is not listed.
3. Page Save commits the mode PATCH, then `reorderProfiles` answers 400 "the collection order is incomplete" (`preferences/collections.rs:198-199`).

A partial failure after deletes also leaves `deletedProfileIds` holding a gone id: the bar says "unsaved changes" with nothing visible.

**Fix:** rebase `profileIds` on the server list (keep the user's relative order, append new ids, drop gone ids), and run the reorder first.

### UI-5. Wizard Preview overwrites concurrent edits

**S2 · Confirmed** for MDBList sources.

- When a preview returns with `sourceIdentity`, `setDraft(previewedDraft)` restores the copy taken at click time (`CollectionSettings.tsx:423-426`).
- Title, description, cadence and Home edits made meanwhile are lost.
- Preview is not disabled during a poster upload (345, 608).

**Fix:** a functional update that patches only `source.listId`, and only while the source still matches.

### UI-6. Smaller UI findings

| Finding | Sev | Status | Evidence |
|---|---|---|---|
| Home settings: a sync that adds or removes a genre or library makes Save fail with 409 on every retry until Discard (the `elements` array is kept whole) | S3 | Confirmed | `home_settings.rs:326-330`, `use-source-draft.ts:8-11` |
| Player Save fails on hidden built-in shortcut conflicts after switching the backend to external mpv | S3 | Confirmed | `PlayerSettings.tsx:134`, `viewing.rs:284-303` |
| Collection wizard closes on Escape or an overlay click and drops a dirty draft without asking | S3 | Confirmed | `CollectionSettings.tsx:490`, 636 |
| Viewing rules validated natively but not in the UI (400 toast; the draft is kept) | S4 | Confirmed | `viewing.rs:174-202`, `ViewingSettings.tsx:94-96` |
| "Restart required" and recovery notices vanish after any Save | S4 | Confirmed | `ApplicationSettings.tsx:201-203`, `settings.rs:197-234` |
| Two quick library-filter changes can save out of order; the older route is remembered | S4 | Plausible | `Library.tsx:229-230`, `accounts.rs:413-427` |
| Collections Save sends deletes, then the settings PATCH, then a reorder (`CollectionSettings.tsx:670-682`). Each request takes a fresh scope (`profiles.rs:184`), so if the account switched between them, the later requests would write account A's draft into account B. The steps are local writes, so the window is milliseconds | S3 | Plausible | `profiles.rs:184-216` |
| Preference change notifications are sent after releasing the lock and can apply out of order | S3 | Plausible | `service.rs:396-425`, `events.rs:607-643` |

---

## 7. Checked and rejected

These candidates were traced and are prevented by the guard named. They are listed so they are not raised again.

**Session and accounts**

- **A late answer or 401 from account A reaching account B:** `confirm`, `Live::open` and `owns_failure` check generation and account (`session.rs:316-329`, 549-560, 614-625).
- **Two 401s sending two expiry events:** the second sees `expired` (`session.rs:561-563`).
- **Lost updates on `accounts.json`, `settings.json` or the deletion journal:** the lock is held across clone, write and swap (`accounts.rs:618-636`, `service.rs:392-417`, `deletions.rs:102-110`).
- **Lock-order deadlock:** the order is always gate → state → preferences → accounts. No `commit_if_current` closure calls back into the session.
- **Network calls under the gate:** `logout` calls the server outside it. `scoped_error` emits after dropping it.
- **Double login submit:** the button is disabled while pending.
- **Overlapping Quick Connect polls:** TanStack dedupes in-flight fetches.
- **Double delete:** every deletion step is idempotent. (A double logout is not covered; see SE-3.)
- **A crash mid-deletion:** the journal resumes it at launch.
- **A crash between the catalog clear and `save_credentials`:** leaves a consistent signed-out, empty state.
- **The previous account's catalog shown after logout and a sign-in as someone else:** the cache-owner stamp plus the switch check (`session.rs:445-458`).
- **The UI showing another account's cached data:** query keys include `accountKey`, and other accounts' roots are removed on switch (`query-client.ts:177-206`).

**Playback**

- **Stale mpv events after a reset:** `active_id == session_id` checks (`events.rs:38-41`, 49-51).
- **Stale segment responses:** `playback_id_is_current` (`segment_skip.rs:21-28`).
- **The replaced file's `end-file(stop)`:** consumed by `replacement_end_file_pending`.
- **Stop during the startup seek losing the resume position:** the held position is reported.
- **`loadfile start` or `#t=`:** not used; the fragment is stripped (`commands.rs:14-16`).
- **Startup `pause=false`:** set in local state only.
- **More than one IPC writer, or pipes reopened per command:** one `IpcCommandWriter` per session (`ipc.rs:140-182`).
- **Locks held across IPC or network:** the snapshot, coordinator and remote handler all clone before calling.
- **Thread shutdown order:** `reset_mpv` stops the runtime before joining the IPC threads.
- **Progress reported for the wrong item or after stop:** progress only from Playing, and the queue dedupes per item.
- **A segment skipped twice:** the `triggered` flag.
- **Remote Play after sign-out:** `scope_is_current` stops a Play that has not passed its check, and the socket closes on scope change. Commands already queued or read in the last tick still run (PB-8).
- **Identical Play pressed twice:** `is_duplicate`.

**Sync and collections**

- **A sync request lost while a cycle runs:** the flag is coalesced (`worker.rs:161-167`).
- **An account switch mid-sync writing old data:** every write goes through `commit_if_current`.
- **Database recreation racing a writer:** `migrate` runs before any thread starts.
- **A cursor advanced without data:** `OFFSET` is written after `ingest_page` in the same closure.
- **A partial identity walk deleting titles via the cap or an error:** both make the walk Incomplete. Only the pagination shift remains (LS-4).
- **The old account's failure shown for the new one:** filtered by `failure.generation`.
- **A stale snapshot after an edit:** readers select by revision.
- **The rating cache poisoned by a failed fetch:** failures write nothing.
- **`retain_ids` racing a socket insert:** both need `&Live` and the gate.
- **SQLite `BUSY_SNAPSHOT` upgrades:** writer transactions start with a write.

**Shell**

- **Double reveal, or reveal before the window exists:** `revealed` and `reveal_requested` handle both.
- **Duplicate update download or mpv install:** one-shot flags.
- **An update notice lost before the page loads:** it is re-sent on `on_load_end`.
- **Sync events lost before the UI bridge mounts:** sync is held until window ready.
- **Locks held across CEF calls:** clone-then-call at every site checked.
- **Services built twice:** `INIT_LOCK` plus a double check.
- **Playback events reordered:** the coalescer flushes state before other events.
- **A cancelled request still applying side effects:** handlers run to completion by design (`api.rs:53-57`). Exceptions found: SE-2 and the abandoned Quick Connect poll in SE-3.
- **A forced close blocked by a dirty draft:** `force_close_requested` continues the unload.
- **Close pressed twice:** `try_close_browser` returns 0 while a close is pending.
- **Interrupted file writes at shutdown:** `atomic_write` and staged renames.
- **The final playstate lost at a normal exit:** the controller flushes with a timeout.
- **Bootstrap retry firing twice:** disabled while pending.

**UI**

- **Double Save, or Save then Discard:** all buttons are disabled while saving.
- **A failed save marking the draft clean:** V and S are unchanged on error.
- **A refetch overwriting a dirty scalar field:** `mergeDraft` keeps changed top-level keys. Only arrays and objects are affected (UI-4, UI-6).
- **Account A's draft saved into account B by a single request:** remount on the account key, `useSourceDraft` reset, native `commit_if_current`. A multi-request Save is not covered (UI-6).
- **Back button, mouse buttons or sidebar links bypassing the guard:** all go through `useBlocker`.
- **Optimistic toggles not rolled back:** watched and favorite are not optimistic; they send absolute values and invalidate on success (`queries.ts:481-505`).
- **Native lost updates on settings files:** the lock is held across the save.
- **A partial Letterboxd save:** each completed step leaves the pending list.
