# MediaFlick collections implementation plan

Status: implemented; automated verification complete, live compatibility matrix pending  
Last updated: 2026-08-26  
Release policy: hard cut, with Desktop and Companion shipped as one compatible change

## 1. Goal

Replace the current collection implementation with two explicit Desktop modes:

- **MediaFlick mode** is the standard mode when TMDB is usable. It ignores Jellyfin BoxSets. It shows automatic Movie Franchises and template-created My Collections.
- **Jellyfin mode** lists the server's existing Jellyfin BoxSets as they are. It does not depend on the Companion collection APIs.

The Desktop owns collection configuration and presentation. The Companion owns TMDB and MDBList credentials, provider requests, provider caching, concurrency, and backoff. Desktop exposes read-only Companion and service availability under Settings. It must not expose credentials, service addresses, setup actions, or raw troubleshooting details.

The storage rule is simple:

- User intent lives in JSON.
- Rebuildable results live in `library.db`.
- Uploaded artwork lives in the custom-art directory and is referenced from JSON.

## 2. Product contract

### 2.1 Modes and navigation

- Store the user's explicit mode choice per Jellyfin server and user.
- Represent "no explicit choice" separately from either mode.
- When TMDB first becomes usable and there is no explicit choice, select MediaFlick mode.
- Never replace an explicit Jellyfin choice.
- MediaFlick remains usable when TMDB is temporarily unavailable if the account has existing MediaFlick configuration or cached results.
- A fresh account without TMDB and without prior MediaFlick state does not expose a usable MediaFlick mode.
- Signed-out users do not see Settings > Collections.

The sidebar is mode-specific:

- MediaFlick mode: **Movie Franchises** and **My Collections**.
- Jellyfin mode: **Collections**.
- Templates never appear in the main sidebar.

### 2.2 Movie Franchises

- Derive franchises automatically from movies owned by the active Jellyfin user.
- Use exact TMDB collection identity. Do not use title matching.
- Show movies only.
- A franchise appears when it has at least one visible owned title and at least two visible total titles after filtering.
- Keep future and undated missing titles hidden by default.
- Always show an owned title, even when its release date is in the future.
- Store `includeUnreleased` as a per-account collection setting in JSON. Default it to `false`.
- Re-evaluate visibility when the local date changes, without requiring a provider refresh.
- Recalculate after a complete Jellyfin sync and when Movie Franchises opens.
- Do not add a user-configurable franchise refresh cadence.

### 2.3 My Collections

- A collection profile is one configured collection created from a template.
- Call it a collection in the UI and `collectionProfile` in stored JSON and code contracts.
- Do not add a manual item-by-item collection builder.
- Do not add reusable setting presets above collection profiles.
- Allow more than one profile from the same template.
- Enforce profile names case-insensitively within one account.
- Preserve explicit user ordering. New profiles go last.
- Store the entire configured profile in JSON. Packaged template changes never rewrite an existing profile.
- Retain template ID and template version only as provenance.

The supported profile actions are:

- Open
- Edit
- Check for updates
- Delete
- Reorder in Settings

Do not add Clone, Archive, Reset, Duplicate, bulk request, or import/export actions.

### 2.4 Collection contents

- Represent the complete upstream result, subject to the configured maximum or All.
- Split titles into Owned and Missing. Owned appears first.
- Preserve source order within each section.
- Expand Missing through 24 titles. Collapse it at 25 or more.
- Remember Missing expansion only for the current view.
- Do not add runtime sorting or watched filters in the first version.
- Mixed profiles keep movies and series interleaved in source order. Series cards receive a small type badge.
- Clicking a missing title opens the normal Discover detail and request flow.
- If Seerr is unavailable, the detail remains usable and request actions are absent or disabled with neutral copy.

### 2.5 Identity and ownership

- Use `(mediaType, tmdbId)` as the canonical title identity.
- If Jellyfin lacks a TMDB ID but has IMDb or TVDB, ask the Companion to resolve the title to TMDB and cache the rebuildable mapping in SQLite.
- Never fall back to fuzzy title-and-year ownership matching.
- Collapse duplicate upstream rows into one title.
- If several accessible Jellyfin items match one canonical title, show one Owned card and let the user choose the local item when opening it.
- Reclassify Owned and Missing immediately after a complete local library update. Do not require a provider refresh.

### 2.6 Permissions and content filtering

- Every authenticated Jellyfin user may use provider-backed collections.
- Only Jellyfin administrators may configure provider credentials in the Companion plugin.
- Filter Owned exactly as Jellyfin filters the signed-in user.
- For users with parental, tag, unrated, or folder restrictions, hide Missing completely. Do not disclose hidden counts.
- Exclude adult provider content for every user.

### 2.7 Provider and outage behavior

- TMDB is required for MediaFlick mode.
- MDBList is optional.
- Keep MDBList templates visible but disabled with `MDBList unavailable` when MDBList is not usable.
- Do not link unavailable providers to credential or Companion setup from Desktop.
- Keep the last successful snapshot after a provider failure.
- Show `Last updated ...` only when refresh is overdue or the latest attempt failed.
- Put Retry beside stale or failed status.
- Keep provider and Companion diagnostics out of collection pages.
- If Jellyfin synchronization fails, show the saved snapshot in configured order with `Ownership unavailable`. Do not show Owned and Missing or enable request actions until a complete sync succeeds.
- If no snapshot exists after a database rebuild, keep the JSON-defined profile visible with `Updating results` or `Results unavailable`.

## 3. Persistence ownership

### 3.1 File map

| Store | Scope | Contents | Rebuildable |
| --- | --- | --- | --- |
| `settings.json` | Device | Application, player, window, logging, and other device settings | No |
| `accounts.json` | Server and user | Appearance and connected public profiles | No |
| `collections.json` | Server and user | Mode choice, collection-wide settings, profile order, and complete collection profiles | No |
| `playback-preferences.json` | Server, user, and Jellyfin item | Saved media source, audio, and subtitle choices | No |
| `pending-deletions.json` | Device operation journal | Account deletions that must resume after a crash | Remove after completion |
| Custom-art directory | Profile | User-uploaded collection posters | No |
| `library.db` | Device cache, rows scoped by server and user where required | Jellyfin catalog, collection snapshots, provider mappings, refresh state, and other caches | Yes |

The active Jellyfin session keeps its current database recreation exception. Collection snapshots and other derived collection rows receive no exception.

### 3.2 Target `collections.json` shape

The exact Rust types may use stronger enums, but the serialized shape should follow this model:

```json
{
  "version": 1,
  "accounts": [
    {
      "serverId": "jellyfin-server-id",
      "userId": "jellyfin-user-id",
      "modeSelection": null,
      "franchises": {
        "includeUnreleased": false
      },
      "profiles": [
        {
          "id": "stable-profile-id",
          "revision": "stable-revision-id",
          "template": {
            "id": "tmdb.discover.popular-movies",
            "version": 1
          },
          "title": "Popular movies",
          "description": "",
          "customPosterId": null,
          "source": {
            "kind": "tmdbDiscover",
            "parameters": {}
          },
          "mediaType": "movie",
          "limit": {
            "kind": "all"
          },
          "ordering": "source",
          "cadence": "daily"
        }
      ]
    }
  ]
}
```

Required rules:

- Key account data by Jellyfin server ID and Jellyfin user ID.
- Use stable opaque IDs for profiles, revisions, and custom artwork.
- Use the array order as the My Collections order.
- Store source configuration as a tagged union rather than raw provider URLs or query strings.
- Permit a validated MDBList public-list ID or canonical public-list URL only in the MDBList selector flow.
- Store source-specific schema versions so one source can evolve without rewriting unrelated profiles.
- Apply defaults while reading supported older shapes.
- Upgrade older supported JSON in memory. Write the current shape only after the user changes that settings area.
- If a file version is newer than the app supports, leave it untouched and open that settings area read-only.
- If one profile is semantically invalid or unsupported, retain it and disable only that profile.
- Manual JSON editing is unsupported. Load once at startup and do not watch files for changes.

### 3.3 JSON write and recovery rules

Apply the same durable write behavior to every settings file:

1. Serialize and validate the complete next document.
2. Write a uniquely named temporary file in the destination directory.
3. Flush the temporary file.
4. Preserve the previous valid document as `.bak`.
5. Atomically replace the primary file.
6. Update in-memory state only after replacement succeeds.

If the primary file cannot be parsed:

- Move it aside with a timestamp.
- Restore the last valid `.bak`.
- Show one local recovery notice.
- If no backup is valid, preserve the broken file and load defaults only for that settings area.
- Never overwrite the broken file with defaults.

### 3.4 Playback preference move

Move `item_playback_preferences` out of SQLite and into `playback-preferences.json`.

- Keep the same server, user, and Jellyfin item identity.
- Store only the source and track identity needed to resolve the user's choice.
- Do not delete preferences merely because an item is temporarily absent from the library.
- Remove them through the explicit local-account deletion flow.
- No released tag contains the current SQLite playback-preference implementation, so this move needs no migration.

### 3.5 Custom artwork

- Store uploaded posters in a durable custom-art directory.
- Store only the opaque artwork ID in `collections.json`.
- Write a new image under a new ID before updating JSON.
- Delete the previous image only after JSON commits.
- If JSON fails, remove the new image.
- Treat references from both the primary JSON and `.bak` as live.
- Delete artwork that has remained unreferenced for seven days.
- Do not support custom backdrops.

### 3.6 Crash-safe account deletion

`Delete local account data` is an explicit destructive action under Settings > Client > Application.

The action must:

1. Name the server and account and require confirmation.
2. Write the server and user identity to `pending-deletions.json`.
3. Remove that account from every primary JSON file and every `.bak`.
4. Remove its collection snapshots and other scoped SQLite rows.
5. Remove its custom artwork.
6. Clear its local Jellyfin session when it is the active account.
7. Clear account-scoped UI queries and return the active account to sign-in.
8. Remove the deletion journal last.

Resume any incomplete deletion on the next launch. Never send a deletion request to Jellyfin.

## 4. Rebuildable SQLite model

Keep one SQLite database. Add collection tables to the existing `library.db` schema and let the current pre-1.0 recreation policy drop them with everything else.

Suggested tables:

- `collection_snapshots`: one committed snapshot header per account, profile, and revision.
- `collection_snapshot_items`: normalized ordered provider titles for a profile revision.
- `collection_refresh_state`: last attempt, last success, latest failure, next due time, and initialized state.
- `franchise_snapshots`: automatic TMDB collection headers per account.
- `franchise_snapshot_items`: ordered franchise membership and display metadata.
- `provider_identity_map`: rebuildable IMDb or TVDB to TMDB mappings by media type.

Snapshot item rows should contain only display and matching data:

- Media type
- TMDB ID
- Title and optional original title
- Year
- Overview
- Release or first-air date
- Source order
- Poster and backdrop references
- Adult flag or proof that adult filtering already ran

Do not store raw TMDB or MDBList responses.

Required database behavior:

- Scope profile and franchise state by server ID and user ID.
- Index profile revision plus source order.
- Index canonical `(mediaType, tmdbId)` identity.
- Commit a complete refresh in one SQLite transaction.
- Never replace a successful snapshot with partial provider results.
- Drop unreferenced profile revisions opportunistically.
- Treat a recreated database as every automatic profile being due immediately.
- Keep all collection tables in the ordinary schema creation path. Do not add protected-table migrations or database backups.

## 5. Cross-store commit protocol

JSON is authoritative, but Preview results live in SQLite. Use profile revisions to make Create and result-changing Edit operations appear atomic.

For a new or result-changing profile save:

1. Validate the draft.
2. Run the required Preview and keep 24 sampled resolved titles plus counts.
3. Allocate a new profile revision.
4. Commit the complete Preview snapshot to SQLite under that revision.
5. Atomically write `collections.json` with the profile pointing at the revision.
6. If the JSON write fails, keep the previous profile and snapshot active and remove the new unreferenced snapshot when practical.

JSON must never point at a snapshot revision that failed to commit. A database recreation may legitimately leave JSON pointing at no current snapshot; the scheduler rebuilds it.

Preview is required after changing:

- Source type
- Source parameters
- Media type
- Result limit
- Result ordering
- Include-unreleased behavior on an exact TMDB collection profile

Preview is not required after changing:

- Title
- Description
- Custom poster
- Refresh cadence
- Profile position in My Collections

When a provider is unavailable, allow only the non-result changes above. Keep result-affecting fields read-only. Delete remains available.

## 6. Companion provider contract

### 6.1 Capability and visibility

- Remove `collections-v1`, `collections-v2`, and `collections-curated-v1`.
- Add `collection-experience-v1` for the replacement contract.
- Keep the overall Companion API version unchanged for Calendar, Ratings, and Seerr.
- Report TMDB and MDBList readiness through the existing machine-readable Companion probe.
- Do not add a Desktop status page for this information.
- Reprobe silently when the app resumes and when Settings > Collections opens.

### 6.2 Credentials

- Keep TMDB and MDBList credential fields in the Companion plugin configuration only.
- Preserve already encrypted Companion credentials during the hard cut.
- Continue accepting supported TMDB v3 and v4 credential shapes.
- Validate credentials with a bounded real provider request when saved.
- Validate existing credentials on first use after plugin startup.
- Do not advertise TMDB as usable until validation succeeds.
- Clear provider-specific caches when a credential is removed.
- Do not clear normalized public cache entries merely because a working credential is replaced.

### 6.3 Provider operations

Define contract tests before choosing final route names. The new contract must support these operations:

- TMDB Discover preview and complete result paging.
- Exact TMDB collection preview and refresh.
- TMDB collection resolution for owned movies.
- TMDB metadata and artwork references for movies, series, and collections.
- IMDb and TVDB to TMDB identity resolution.
- MDBList public-list search.
- MDBList public-list validation by ID or canonical URL.
- MDBList public-list preview and complete result paging.
- Provider artwork proxying so Desktop never calls TMDB or MDBList directly.

Return normalized provider titles rather than raw upstream payloads. Each title must carry canonical identity, type, display metadata, order, and artwork references.

MDBList rules:

- Public lists only.
- Do not support private-list tokens or share tokens.
- Collapse forbidden, private, missing, and not-found responses to `List not available`.
- Do not expose server quota counters in Desktop.

### 6.4 Provider execution

- The Companion owns request concurrency, cache freshness, backoff, and retry timing.
- The Desktop notices due work and asks. It does not enforce a second provider quota.
- Use Jellyfin's preferred metadata language and country as TMDB defaults.
- Let a profile override language or region only when its template exposes that parameter.
- Adult content must be excluded before returning provider results.
- All authenticated users may call collection provider operations. Credential management remains administrator-only.

## 7. Template catalog

Package the template catalog with Desktop. Do not store templates in the Companion.

The first catalog contains 122 templates:

- 99 retained baseline templates after excluding six Trakt templates and one blank placeholder, while keeping ten real TMDB franchise templates.
- 23 Series Discover counterparts.

Keep this category order:

1. Trending
2. Popular
3. Streaming Services
4. Top Rated
5. In Theaters
6. Upcoming
7. On Air
8. Editorial
9. Custom

Supported source families:

- TMDB Discover
- Exact TMDB Collection
- MDBList public list

Template implementation rules:

- Reproduce the agreed Silo Server behavior with an independent implementation.
- Do not copy AGPL source code, text, or artwork.
- Give every packaged template a stable ID and version.
- Keep template art in Desktop resources.
- Existing profiles retain copied values when a packaged template changes.
- If a future app removes a source implementation, keep affected profiles visible with their last snapshot. Allow Delete, but disable Edit and Refresh.

The one-screen wizard supports:

- Title
- Description
- One optional custom poster
- Source parameters
- Movie, Series, or Mixed media type where supported
- Maximum result count or All
- Result ordering
- Manual, Daily, Weekly, or Monthly cadence
- Exact TMDB collection include-unreleased option, default off

Preview behavior:

- Preview is explicit and required before Create.
- Show 24 sampled resolved titles and full counts.
- Invalidate Preview after any result-affecting change.
- Creating or saving commits only after validation and a successful required Preview.

## 8. Desktop implementation

### 8.1 Rust domain modules

Add a dedicated collection domain under `src/collections/`. Keep files focused instead of growing `src/companion/mod.rs` or `src/shell/cef/api/collections.rs` further.

Suggested modules:

- `model.rs`: modes, profiles, templates, normalized titles, snapshots, and refresh state.
- `profiles.rs`: account-scoped JSON profile operations and validation.
- `templates.rs`: packaged catalog loading and source-specific draft validation.
- `snapshots.rs`: SQLite snapshot repository and revision cleanup.
- `matching.rs`: canonical identity, Owned/Missing classification, and duplicate local-item handling.
- `franchises.rs`: automatic franchise calculation and release-date filtering.
- `scheduler.rs`: active-account due work and provider backoff coordination.
- `artwork.rs`: custom-art references and orphan collection.

Keep JSON file adapters in `src/preferences/` when they are shared with the settings service:

- Extend `accounts.rs` only for small account preferences.
- Add a collection configuration store for `collections.json`.
- Add a playback preference store for `playback-preferences.json`.
- Add the deletion journal and startup resumption path.
- Reuse one public account key type rather than duplicating server and user identity types.

### 8.2 Companion client

Refactor `src/companion/mod.rs` so collection code consumes only the new provider contract.

- Remove old derived, native, and curated collection methods.
- Keep Calendar, Ratings, and Seerr behavior intact.
- Add normalized provider operations behind `collection-experience-v1`.
- Keep silent readiness state in memory.
- Do not return user-facing strings that mention the Companion.

### 8.3 Shell API

Rewrite `src/shell/cef/api/collections.rs` around the new domain. The UI needs operations for:

- Effective mode and provider readiness
- Mode and franchise setting updates
- Template catalog and template availability
- Preview
- Profile create, list, read, edit, reorder, refresh, and delete
- Movie Franchises list and detail
- My Collections list and detail
- Jellyfin BoxSet list and detail
- Multiple-local-item resolution
- Local-account deletion

Account identity must come from the authenticated Desktop session. Do not accept arbitrary server or user IDs from UI payloads.

### 8.4 Scheduler

- Run only while Desktop is running.
- Process only the active Jellyfin account.
- Pause automatic MediaFlick work while Jellyfin mode is active.
- Keep explicit Preview, Create, Edit, and Check for updates working from Settings when providers are usable.
- On return to MediaFlick mode, queue overdue work.
- After database recreation, wait for a complete Jellyfin sync.
- Prioritize the currently visible collection, then automatic franchises, then My Collections in configured order.
- Resume unfinished work on the next app run.

Cadence rules:

- Manual never schedules automatically.
- Daily is 24 hours after the last success.
- Weekly is seven days after the last success.
- Monthly is the same calendar day in the next month, clamped to its final day.
- A failed attempt does not move the last-success timestamp.
- Companion backoff governs retries.

### 8.5 Library integration

- Include server and user identity in all collection query keys.
- Clear account-scoped collection queries on logout, account deletion, and account change.
- Invalidate Owned/Missing classification after a complete library sync and relevant library events.
- Do not expose partially synchronized ownership.
- Keep previous snapshots visible during ordinary provider failures.
- Fetch Jellyfin BoxSets directly through the normal Jellyfin/library path. Jellyfin mode must not require collection capabilities from the Companion.

## 9. UI implementation

### 9.1 Routes and sidebar

Replace the current single collection route with mode-aware routes. A workable route scheme is:

- `/collections/franchises`
- `/collections/franchises/:tmdbCollectionId`
- `/collections/mine`
- `/collections/mine/:profileId`
- `/collections/jellyfin`
- `/collections/jellyfin/:boxSetId`

`/collections` redirects to the effective mode's first page. A mode change redirects away from a route that no longer belongs in the active sidebar.

Update:

- `ui/src/App.tsx`
- `ui/src/components/AppSidebar.tsx`
- `ui/src/lib/navigation.ts`
- Collection query keys in `ui/src/lib/queries.ts`

### 9.2 Settings > Collections

Add a signed-in Collections settings area with this order:

1. General
2. Configured My Collections
3. Template catalog

General contains:

- Mode selector
- Include unreleased titles in Movie Franchises, default off

Configured My Collections supports:

- Reorder
- Edit
- Delete with confirmation
- Provider and originating-template labels
- Read-only unsupported state

The template catalog supports:

- Agreed category ordering
- Search
- Provider availability states
- Add flow into the one-screen wizard

Keep source labels in Settings only. Do not add TMDB or MDBList badges to main collection cards or detail pages.

### 9.3 Collection pages

Movie Franchises:

- Show automatic franchise cards.
- Use exact TMDB collection identity in routes and query keys.
- Show `No movie franchises found.` when complete and empty.
- Show `Finding movie franchises...` while rebuilding.

My Collections:

- Show profiles in JSON order.
- Show `No collections yet` and a `Choose templates` link to Settings > Collections.
- Keep the page focused on browsing.
- Put Check for updates and an Edit shortcut in the card menu. Edit opens the matching settings entry.

Jellyfin Collections:

- Show existing BoxSets without importing, changing, or mirroring them.
- Show `No Jellyfin collections found.` when empty.

Detail pages:

- Use Owned and Missing when ownership is trustworthy.
- Use an ungrouped snapshot with `Ownership unavailable` after a failed full Jellyfin sync.
- Preserve source order.
- Collapse Missing at 25 titles.
- Show a Series badge in Mixed profiles.
- Remove runtime sorting and watched filters.
- Show stale status only when overdue or failed.
- Update an open page when an automatic snapshot refresh commits.

### 9.4 Companion UI boundary

Keep Companion-specific information in the read-only Settings integration page. It reports plugin compatibility, version, service availability, and the signed-in user's Seerr mapping without accepting configuration.

Remove Companion references from other Desktop screens, including:

- Companion version text in `ui/src/components/AppSidebar.tsx`.
- `via MediaFlick Companion` rating attribution in `ui/src/components/RatingOverlay.tsx`.
- Companion setup/status links and wording in Seerr gates and cast discovery.

Use provider or service names only where they help the user:

- Ratings may say `via MDBList`.
- Seerr failure copy may say `Seerr is unavailable for this account.`
- Offer Back to Home rather than a Companion setup link.

## 10. Companion plugin implementation

### 10.1 Keep

- TMDB and MDBList credential fields and encrypted storage.
- Seerr gateway behavior.
- Ratings behavior and cache.
- Calendar behavior.
- Admin-only configuration page.
- Existing non-collection capabilities and API version.

### 10.2 Replace

- Replace collection-specific methods in `Api/CompanionControllers.cs` with normalized provider operations.
- Refactor reusable TMDB request and cache code out of `Services/CollectionsService.cs` before deleting the old collection contract.
- Add explicit TMDB validation and readiness state.
- Add MDBList public-list search, validation, and complete paging.
- Add artwork proxying.
- Add provider response normalization and adult filtering.
- Register the replacement provider services in `ServiceRegistrator.cs`.

### 10.3 Remove in the hard cut

- Collection definition and native-sync fields from `Configuration/PluginConfiguration.cs`.
- Collection mirroring and curated-collection controls from `Configuration/configPage.html`.
- Old collection controllers and response models.
- `Services/CuratedCollectionResolver.cs`.
- `Services/NativeCollectionSync.cs`.
- `ScheduledTasks/CollectionsSyncTask.cs`.
- Old collection cache registration and obsolete cache files.
- Old collection service code that is not reused by the provider gateway.
- Tests that assert the removed derived, native, or curated contracts.

Do not delete or alter existing Jellyfin BoxSets. Do not import old curated definitions. Do not transfer credentials to Desktop.

## 11. Implementation phases

No intermediate build should be released. New provider operations may coexist with old code during development, but Phase 8 removes the old contract before release.

### Phase 1: Lock contracts and fixtures

- [ ] Add shared fixture documents for collection profiles, templates, normalized provider titles, snapshots, and readiness.
- [ ] Define `collection-experience-v1` contract tests in Rust and C#.
- [ ] Define stable error categories without provider secrets or Companion wording.
- [ ] Record the 122-template manifest and category counts in a deterministic test.
- [ ] Add license provenance notes for independently created template data and artwork.

Exit criteria:

- Rust and C# tests agree on serialized provider title and error shapes.
- Template IDs and versions are fixed before profile storage ships.

### Phase 2: Finish JSON persistence

- [ ] Generalize atomic JSON writing and `.bak` recovery.
- [ ] Add `collections.json` with account scoping, validation, lazy upgrades, and read-only newer-version handling.
- [ ] Add `playback-preferences.json` and move the current SQLite playback preference service.
- [ ] Add `pending-deletions.json` and startup resumption.
- [ ] Add custom-art storage and seven-day orphan cleanup.
- [ ] Add explicit local-account deletion to the settings service and shell API.
- [ ] Test duplicate accounts, duplicate profile names, invalid single profiles, malformed primary files, valid backups, newer versions, and interrupted deletion.

Exit criteria:

- Every user-configured collection and playback value survives a `library.db` recreation.
- No user-configured setting remains in a rebuildable SQLite table.

### Phase 3: Add rebuildable collection storage

- [ ] Bump the ordinary library schema and add snapshot, refresh, franchise, and identity mapping tables.
- [ ] Add normalized snapshot repositories and transaction tests.
- [ ] Add profile revision cleanup.
- [ ] Add canonical identity matching and secondary-ID resolution cache interfaces.
- [ ] Test database recreation with JSON profiles present.
- [ ] Test that partial provider results never replace a successful snapshot.

Exit criteria:

- Deleting and recreating `library.db` loses only rebuildable data.
- A JSON profile without a snapshot appears as due and rebuildable.

### Phase 4: Build the Companion provider gateway

- [ ] Add bounded credential validation at save and first use.
- [ ] Add TMDB Discover, exact collection, franchise resolution, ID mapping, and artwork operations.
- [ ] Add MDBList public search, validation, paging, and normalized errors.
- [ ] Add shared provider caching, concurrency, and backoff.
- [ ] Add `collection-experience-v1` readiness.
- [ ] Add authorization tests for authenticated users and admin-only credential changes.
- [ ] Add tests proving that private or forbidden MDBList lists return `List not available`.
- [ ] Add tests proving that logs and responses never contain provider credentials.

Exit criteria:

- Desktop can build every template and franchise response without calling TMDB or MDBList directly.
- TMDB readiness is false until a real validation succeeds.

### Phase 5: Add Desktop collection orchestration

- [ ] Add the Rust collection domain modules.
- [ ] Replace old Companion collection calls with normalized provider operations.
- [ ] Implement preview, revisioned create/edit, refresh, and delete.
- [ ] Implement active-account scheduling and mode pause behavior.
- [ ] Implement automatic franchises and local-date visibility changes.
- [ ] Implement Owned/Missing matching, duplicate local-item selection, restrictions, and adult exclusion defense.
- [ ] Add account identity to collection query and invalidation keys.
- [ ] Add failure-state tests for provider outage, failed Jellyfin sync, database recreation, stale snapshots, and unavailable sources.

Exit criteria:

- The full collection domain is testable without the React UI.
- MediaFlick configuration remains intact through logout and database recreation.

### Phase 6: Package templates and build Settings

- [ ] Add the 122-template catalog and independently created assets under Desktop resources.
- [ ] Add General, configured profile, and template sections to Settings > Collections.
- [ ] Add the one-screen wizard and required Preview behavior.
- [ ] Add reordering, edit, delete, unsupported, and provider-unavailable states.
- [ ] Add custom poster upload and removal.
- [ ] Add UI tests for every result-affecting Preview invalidation.
- [ ] Add UI tests for case-insensitive name uniqueness and same-template reuse.

Exit criteria:

- A user can configure every supported collection without leaving Desktop settings.
- No manual item-by-item collection path exists.

### Phase 7: Build the browsing experience

- [ ] Add mode-aware routes and sidebar entries.
- [ ] Build Movie Franchises list and detail.
- [ ] Build My Collections list and detail.
- [ ] Rework Jellyfin Collections to use direct BoxSet data.
- [ ] Add Owned/Missing, Mixed badges, collapsed Missing, stale state, ownership-unavailable state, and empty states.
- [ ] Connect missing titles to Discover and Seerr request behavior.
- [ ] Add local-item selection for duplicate owned editions.
- [ ] Remove runtime sorting and watched filters from the replacement detail page.

Exit criteria:

- Both modes work without old Companion collection capabilities.
- Restricted users never receive or infer hidden Missing titles.

### Phase 8: Perform the hard cut

- [ ] Remove old C# collection definitions, native mirroring, curated resolution, scheduled task, configuration, models, endpoints, caches, and registrations.
- [ ] Remove `collections-v1`, `collections-v2`, and `collections-curated-v1` from Companion discovery.
- [ ] Remove old Rust derived, native, and curated collection client methods.
- [ ] Remove or completely rewrite the old shell collection API.
- [ ] Remove the old React collection route implementation and tests.
- [ ] Keep the read-only Companion status route and remove Companion setup links and wording from feature screens.
- [ ] Remove direct Desktop TMDB artwork requests.
- [ ] Delete obsolete plugin cache files on upgrade without touching Jellyfin BoxSets.
- [ ] Confirm that existing encrypted TMDB and MDBList credentials still load.

Exit criteria:

- Searching the tree finds no old capability strings or native/curated collection configuration.
- Existing Jellyfin BoxSets remain unchanged.

### Phase 9: Documentation and release verification

- [ ] Update `README.md`, `plugin/README.md`, and `CHANGELOG.md`.
- [ ] Document that Desktop JSON files are app-owned and manual editing is unsupported.
- [ ] Document the hard cut and lack of legacy definition import.
- [ ] Verify clean upgrade behavior with working Companion credentials.
- [ ] Verify the Companion status page reports unavailable or incompatible plugins without offering setup or credential controls.
- [ ] Verify the plugin dashboard remains the only credential-management UI.
- [ ] Run the full automated and manual test matrix below.

## 12. Verification gates

Run these deterministic checks before each phase merge that touches the relevant component:

```powershell
just fmt-check
just clippy
just test
pnpm --dir ui lint
pnpm --dir ui test
pnpm --dir ui build
just plugin-test
just plugin
```

Also run:

```powershell
git diff --check
python scripts/check_rust_file_size.py
```

### 12.1 Required persistence tests

- Create two accounts on one server and the same user ID on two different servers. Verify isolation.
- Recreate `library.db`. Verify JSON settings, profiles, order, playback choices, and custom art survive.
- Verify snapshots disappear and rebuild.
- Interrupt every JSON save point. Verify primary or backup recovery.
- Interrupt account deletion after each step. Verify startup completes it.
- Load a newer JSON version. Verify read-only behavior and byte-for-byte preservation.
- Corrupt one profile semantically. Verify other profiles still work.

### 12.2 Required collection tests

- Alien produces its exact TMDB franchise from an owned movie.
- F1 does not appear when its only second member is unreleased and the option is off.
- Enabling unreleased titles makes the qualifying F1 franchise appear.
- A cached title becomes visible at the local date boundary without a provider refresh.
- An exact TMDB Collection profile applies its own include-unreleased option.
- A public MDBList URL and its list ID resolve to the same source identity.
- A private MDBList list returns `List not available`.
- A Mixed profile preserves source order inside Owned and Missing.
- Missing collapses at 25 and stays expanded at 24.
- A provider refresh failure keeps the previous complete snapshot.
- A failed Jellyfin sync removes Owned/Missing classification and disables requests.
- Multiple Jellyfin editions produce one Owned title with a local-item chooser.
- IMDb and TVDB fallback resolve through TMDB without fuzzy matching.

### 12.3 Required permission tests

- An unrestricted authenticated user can preview and refresh provider collections.
- A non-admin cannot read, change, or validate provider credentials.
- A restricted user receives filtered Owned titles and no Missing titles or hidden counts.
- Adult titles never enter Preview or committed snapshots.
- Account A cannot read Account B's profiles, snapshots, custom art, or refresh state.

### 12.4 Required UI tests

- The sidebar changes correctly between MediaFlick and Jellyfin modes.
- An explicit Jellyfin choice survives later TMDB availability.
- No explicit choice changes to MediaFlick when TMDB first becomes usable.
- Templates appear only in Settings > Collections.
- MDBList templates remain visible but disabled when unavailable.
- Presentation-only edits save without Preview.
- Result-affecting edits cannot save without a fresh Preview.
- My Collections order survives restart.
- Empty, rebuilding, stale, failed, unsupported, and ownership-unavailable states use the approved copy.
- Desktop contains one read-only Companion status page and no Companion setup link, credential control, or service address.

### 12.5 Manual compatibility matrix

| Desktop state | Companion state | Expected behavior |
| --- | --- | --- |
| Fresh account | TMDB valid | MediaFlick becomes the default |
| Fresh account | TMDB unavailable | Jellyfin works; MediaFlick has no usable provider-backed state |
| Existing MediaFlick account | Provider outage | JSON profiles and last snapshots remain visible |
| Existing MediaFlick account after DB recreation | Provider outage | Profiles remain; results show unavailable until rebuild |
| Explicit Jellyfin account | TMDB becomes valid | Stay in Jellyfin mode |
| Jellyfin mode | MDBList unavailable | Jellyfin works; MDBList templates are disabled in Settings |
| Restricted account | Providers valid | Owned is Jellyfin-filtered; Missing is absent |
| Old or incompatible Companion | New Desktop | Jellyfin mode works; no Companion troubleshooting UI appears |

## 13. Definition of done

The implementation is complete only when all of these statements are true:

- MediaFlick and Jellyfin modes are separate and never mix their collections.
- Movie Franchises derives automatically from exact TMDB collection identity.
- My Collections contains only template-created profiles.
- Every user-configured persistent value lives in JSON or referenced custom-art files.
- Every collection result, mapping, timestamp, and refresh state in SQLite can be discarded and rebuilt.
- Desktop makes no direct TMDB or MDBList request.
- Desktop has no provider credential form or Companion setup UI. Its read-only Companion page reports compatibility and service availability.
- Companion credentials remain usable after the hard cut.
- The old collection capabilities, configuration, scheduled task, mirroring, curated definitions, and caches are gone.
- Existing Jellyfin BoxSets are untouched.
- All automated gates and the manual compatibility matrix pass.
