# Support every Jellyfin library

Routed to figure-it-out (cross-cutting: sync, schema, API, UI, settings, Home).

- [x] Read the Principles section of the poteto-mode skill.
- [x] Phase A: Frame (see Framing below)
- [ ] Phase B: Design the workflow (architect over the whole design, then the phase list)
- [ ] Phase C: Run the loop
- [ ] Phase D: Keep the audit trail (.audit/jellyfin-libraries.tsv)
- [ ] Phase E: Verify and hand back

Earlier Feature-playbook step 1 (`how`) is done: 3 explorers + live probes, grounding in .audit/jellyfin-libraries.grounding.md.

## Framing

- Done predicate: in the real app against the owner's server and the demo server, (1) the sidebar lists every library /UserViews returns, by name, in Jellyfin's order; (2) each library opens a browse grid of its items; movies/tvshows libraries keep today's filters, sorts and detail features scoped to that library; (3) any other item opens a generic details page with no TMDB/Letterboxd; (4) Home settings list a "Latest in <library>" shelf per library and Home renders the enabled ones; (5) Search, Favorites and My list return no Season or Episode rows. Each proven by a verify-mediaflick drive with saved artifacts.
- Scope: Rust sync + schema + query + API, UI sidebar/library/detail/home/settings, accounts.json formats (browsing keys, startup destination, home elements). Roughly 4 stacked PRs.
- Rigor: high. accounts.json format changes are durable user data (one-way-ish). Schema bump forces a full re-sync for every user.
- Blockers: cross-family review unavailable (only Claude enabled), so independent-review gates run Claude-only and get flagged.

## Throughput checkpoint

- [ ] Blocking first steps: PR0 (search fix) fix round + review; PR1 (registry, schema 19, membership pass, /api/libraries, catalog-tier library routes) is the base every later PR reads.
- [ ] Independent workstreams: none worth parallelizing at code level. PR2-PR5 all touch catalog.rs, api types.ts and Library/Home routes; Feature playbook says code-coupled work goes to one owner at a time. Probing unmeasured library types (books/photos/homevideos) is independent and needs a server that has them.
- [ ] Shared mutable state: catalog.rs, ui/src/lib/api/types.ts, accounts.rs. Split by stacking: each PR in its own worktree branched from the previous PR's head; one writer per worktree.
- [ ] Smallest safe decomposition: stack PR0 -> PR1 registry/membership -> PR2 generic details -> PR3 browse+sidebar+live tier -> PR4 startup destination -> PR5 Home shelves. One fresh owner per PR, each ending in a verify drive artifact, interrogate round per PR.

## Phase B design: done (.audit/jellyfin-libraries.design.md, base C2 + 9 amendments)

## Phase C units
- [x] PR0 fix/search-favorites-top-level-only: #275 open, 3 review rounds + 3 verifier passes; BLOCKED: independent review (Claude-only panel). PR1 must rebase onto its tip 4b6c7ab.
- [x] PR1 feat/library-registry: #276 open, 3 review rounds + 4 verifier passes; BLOCKED: independent review
- [x] PR2 feat/generic-item-details: #277 open, 3 review rounds + final verifier (both mpv backends); BLOCKED: independent review. Carry to PR3: live-item refresh signal, playlist order, delete-titles-of-removed-library
- [ ] PR3 feat/library-browse
- [ ] PR4 feat/library-startup-destination
- [ ] PR5 feat/library-home-shelves
