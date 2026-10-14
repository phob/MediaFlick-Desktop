# Companion: Releases, ratings and status

With the MediaFlick Companion installed on the Jellyfin server, MediaFlick shows Sonarr and Radarr release dates on the Releases page and in Home's Releases shelf, MDBList ratings on cards and detail pages once the user picks rating sources, and a read-only status page under Settings → Integrations. Without the Companion's `calendar` capability, Releases falls back to Jellyfin's upcoming episodes; without `ratings-v1` the rating-source picker is disabled.

## Sub-features

- `companion-status` shows the plugin connection, feature compatibility and each service (Seerr, Sonarr, Radarr, MDBList, TMDB) as available or not. It has no save bar and does not show the plugin version.
- `releases-page` shows a month's releases (`/calendar`) as an Agenda or a Month grid, with month navigation and toggles for Episodes, Digital, Cinema and Physical releases.
- `releases-shelf` is Home's Releases shelf; see [Home](./home.md).
- `ratings` shows MDBList ratings for the sources picked in Settings → Appearance → Rating sources, on library and Home cards, the hover preview, Discover cards and the detail hero.

## How to get to it (user POV)

- Sidebar `Releases` (always shown when signed in).
- Settings → `Settings navigation` → Integrations → `MediaFlick Companion`.
- Settings → `Settings navigation` → Account → `Appearance` → Rating sources.

## Driving it with ctx (just verify)

Preconditions: your own server with the Companion, `just verify companion --account SERVER`, after the first sync.

- **Probe.** `ctx.api("/api/companion/info")` returns `available`, `compatible`, and `info.pluginVersion`, `info.capabilities`, `info.services`. Jellyfin 12.2 with Companion 0.2.2 advertised `collection-experience-v1`, `franchise-memberships-v1`, `calendar`, `seerr`, `seerr-person-discovery`, `seerr-discovery-v2`, `seerr-discovery-v4`, `seerr-request-profiles` and `ratings-v1`. *Proven by `companion` (`companion.json`).*
- **Status page.** `{ role: "link", name: "MediaFlick Companion" }` opens `/settings/integrations/companion` with `h3` rows `Connection`, `Desktop features`, `Seerr`, `Sonarr`, `Radarr`, `MDBList`, `TMDB`, each followed by a status label: `available`, `compatible`, `unavailable` or `missing` in the DOM, upper-cased by CSS, and the AX tree reads the upper-cased text (`AVAILABLE`, `COMPATIBLE`). `Desktop features` shows only when the Companion is compatible and no feature is missing; otherwise the rows name what is missing (`Companion version`, `Movie franchises`, `Cast discovery`, `Release-decade filters`, `Request profile selection`), and without a Companion the whole card is hidden (`CompanionSettings.tsx`). The Seerr row names the mapped user (`This account is mapped as Testuser.`). Only that link carries `data-active="true"` in Settings navigation; none carries `aria-current`. *Proven by `companion` (`01-companion-status`).*
- **Releases.** Sidebar `Releases` opens `/calendar`; `main h1` is the month (`October 2026`), the `Agenda` tab is selected, and entries are `main article`s with `Play <title>`, `Details` or `Series details`, or `Request options` for an unowned title with Seerr. The type toggles are plain buttons without `aria-pressed`: pressing `Episodes` hid the episode entries (56 → 11) and pressing it again restored them. The `Month` tab shows 42 `[data-calendar-cell]` days; `Next month` and `Today` move the heading. Read back with `ctx.api("/api/calendar?start=YYYY-MM-DD&end=YYYY-MM-DD")`. *Proven by `companion` (`02-releases-agenda`, `03-releases-month`).*
- **Ratings.** In Appearance, the Rating sources fieldset holds checkboxes named by source (`IMDb`, `Rotten Tomatoes Critics`, `MDBList Score`, …). Check two and `Save`: `/api/settings` `appearance.ratingSources` holds their ids, and Movies cards gain `dl[aria-label="Ratings for <title>"]` whose entries are named `<Source> rating <value>` (`IMDb rating 7.3 out of 10`, `Rotten Tomatoes Critics rating 85 percent`), only for the picked sources. `Restore defaults` and `Save` clear them. *Proven by `companion` (`05-movie-ratings`, `06-detail-ratings`).*
- **Empty and error states.** `No releases in this window`; `Could not load releases` with `Try again`; `<source> unavailable. Showing the last cached releases.` Source-only.

## Gotchas

- Never press `Play <title>` (playback) or `Request options` (a Seerr write) on a Releases entry outside the drives that own those paths.
- Ratings are off by default: a fresh profile shows no readouts until a source is saved. `Sort: Rating` uses Jellyfin's community rating, never MDBList.
- The server's MDBList quota is shared by every user (free tier 1,000 a day); the plugin caches ratings for 7 days and Desktop for 24 h, so repeated runs mostly hit caches.
- Calendar dates are plain dates; Today and the month grid use the local time zone, so an entry near midnight can sit a day off from Sonarr or Radarr.
- Spoiler protection (on by default) shows the series title instead of the episode title on Releases.
- The agenda includes the grid's leading and trailing days from the neighbouring months.
