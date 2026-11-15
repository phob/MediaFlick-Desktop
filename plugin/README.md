# MediaFlick Companion

The server-side companion for MediaFlick Desktop. It exposes typed,
Jellyfin-authenticated endpoints under `/MediaFlick`, keeps provider
credentials on the server, and runs Seerr calls as the mapped Jellyfin user.
There is no arbitrary proxy endpoint.

The plugin targets Jellyfin 12.0.0 (`net10.0`) and builds with the .NET 10 SDK
pinned in the repository's `global.json`. Build it with `just plugin`, test it
with `just plugin-test`, and deploy the release publish output to the configured
development server with `just plugin-deploy`.

## Jellyfin compatibility

Jellyfin reads a plugin's `targetAbi` as the minimum server version. The
Companion compiles against the oldest Jellyfin it supports, so one build loads
on that release and every later release with the same major version. A build
compiled against newer packages could call APIs that older servers lack and
would fail at runtime there.

- The `Jellyfin.Controller` version in the plugin project is the floor.
  `package.py` derives `targetAbi` from it, so the two cannot drift.
  Desktop's `MINIMUM_SERVER` in `src/companion/mod.rs`, which tells users on an
  older server that the Companion needs a newer Jellyfin, has a Rust test that
  reads the same reference.
- Renovate proposes only new major Jellyfin packages. Raise the floor by hand,
  in one change to every Jellyfin package reference, and only when the plugin
  needs a newer API.
- Before raising the floor, keep earlier catalog entries in the repository
  manifest. Jellyfin then offers older servers the newest build they can load.
- `just plugin-e2e` downloads the official portable server for the floor and
  for the newest minor release. On each clean server it reads the committed
  repository manifest, installs the packaged plugin from a loopback
  repository through Jellyfin's catalog, restarts, and calls
  `/MediaFlick/info`. Pass versions such as `12.0 12.2` to choose them. The
  report and server logs go to `build/e2e/companion-jellyfin/`.

## Install

Administrators install the Companion from its Jellyfin plugin repository:

```text
https://flick.media/companion/manifest.json
```

In Jellyfin 12.0.0 or newer, open **Dashboard → Plugins → Repositories**, add
this URL with the name **MediaFlick Companion**, install **MediaFlick
Companion** from the catalog, and restart Jellyfin. Desktop's **Settings →
MediaFlick Companion** then reports the plugin. Configure provider services in
Jellyfin's plugin dashboard. The same steps are on
[flick.media/companion](https://flick.media/companion/), which Desktop links to
when a server has no Companion.

The manifest is `website/public/companion/manifest.json`. Its entries point
only at published GitHub Release assets: Jellyfin downloads without signing
in, so a draft's asset would answer 404. Until the first Companion release is
published, the manifest is an empty list, which Jellyfin reads without error
(a missing manifest logs an error on every catalog refresh).

## Release

Companion versions are independent of Desktop versions and must be three or
four numbers; Jellyfin accepts no suffix such as `-test.1`. Compatibility uses
the advertised API version and capabilities, plus the package's Jellyfin ABI.
`plugin/build.yaml` names the next release; `just plugin-test` checks that its
`version` matches `Directory.Build.props` and its `targetAbi` matches the
`Jellyfin.Controller` floor.

1. Run **Companion Release** in GitHub Actions on `main` with step `draft` and
   the version. It tests, stamps and verifies the DLL version, packages the
   plugin with `package.py build`, and creates a draft release
   `companion-v<version>` excluded from Latest.
2. Test the draft: download its ZIP and extract it into a test server's
   `plugins/MediaFlick Companion_<version>` directory, or run
   `just plugin-e2e` on the same commit.
3. Publish the draft with **Set as the latest release** unchecked. The
   desktop updater and the website's download buttons read GitHub's Latest
   release, so a Companion release there would stop Desktop updates. The
   **Latest Release Guard** workflow runs on every published or edited release
   and puts Latest back on the newest desktop release, failing red when it had
   to.
4. Publishing runs the workflow's `repository` step. `package.py add`
   downloads the published ZIP anonymously, takes the entry's checksum from
   those bytes, keeps earlier versions and refuses to replace a listed one. It
   pushes the result to `companion-repository/v<version>` and the run summary
   links a pull request to open. If the release event did not run it, run
   **Companion Release** with step `repository` and the version.
5. Merge that pull request. Cloudflare deploys the website, and servers with
   the repository offer the update.

## MDBList and TMDB administrator setup

Open **Dashboard → Plugins → MediaFlick Companion**. MDBList and TMDB keys are
optional and have independent **Save / replace**, **Validate**, and **Remove**
actions. Inputs are password fields and saved values are never filled back into
the page or returned by an API. Purpose-bound ASP.NET Data Protection ciphertext
is stored in Jellyfin's plugin configuration; its persistent key ring lives in
the plugin data directory with owner-only modes where the host filesystem
supports them.

MDBList is validated through the inexpensive fixed-origin `/user` endpoint.
The page distinguishes valid, rejected, unreachable, and rate-limited states
and shows only non-secret quota/retry facts. TMDB credentials accept supported
v3 and v4 shapes and are verified with a bounded provider request before the
collection capability becomes available.

Desktop reads MDBList ratings only through the Companion's authenticated
contract. The administrator key stays on the Jellyfin server and is never
returned to Desktop. MDBList OAuth is intentionally not used because the
official API documents the same account-tier rate limits for API keys and
OAuth.

## Ratings v1 contract

Authenticated clients probe `GET /MediaFlick/info` (Companion API version 1).
When the saved MDBList key is valid, the response advertises `ratings-v1` and a
non-secret `ratings` capability object containing:

- ratings boundary version/range (`1`), server ownership metadata, and
  server-key validity;
- the supported source catalog, including Letterboxd and separate Rotten
  Tomatoes critic/audience entries;
- quota, retry, validation timestamp, and TMDB validation status—never an API
  key, bearer token, or reusable credential.

Desktop v1 then posts its exact versioned payload to
`POST /MediaFlick/ratings/v1/batch`:

```json
{
  "boundaryVersion": 1,
  "items": [
    {
      "itemId": "jellyfin-item-id",
      "kind": "Movie",
      "mediaType": "movie",
      "provider": "tmdb",
      "providerId": "603"
    }
  ]
}
```

The endpoint requires normal Jellyfin authentication, accepts at most 500
strict TMDB/IMDb identities, and uses only MDBList's fixed host and allowlisted
media-info batch paths. Upstream groups contain at most 100 IDs and are
serialized through one shared refresh gate.

Results are keyed back to each requested `itemId`, with normalized source
identifiers, source update/fetch timestamps, `server_mdblist` origin, and stale
metadata. Missing ratings are omitted without failing the batch. Ratings from
sources outside the fixed catalog are dropped, so no upstream source text is
returned.

## Shared quota and cache behavior

The administrator's MDBList quota is shared by every Jellyfin user (the
published free tier is 1,000 requests/day), so the plugin maintains one
server-wide, stable-ID cache—not a cache per client or card. Entries are fresh
for seven days (negative results for one day), remain
stale-servable for 30 days, and are persisted atomically in the plugin data
directory. Changes are batched into one write about two seconds later and
written on shutdown; expired entries are dropped and the file holds at most
100,000 titles. Stale entries return immediately while one deduplicated background
refresh runs. Concurrent cache misses re-check behind the shared gate, and
MDBList receives media-info batches rather than one request per item or source.

`X-RateLimit-*` and `Retry-After` are persisted with exponential transient
backoff, so restarts do not create a retry storm. HTTP calls have connect,
per-request, body-size, and foreground-refresh bounds and honor cancellation.
Provider failures affect only the optional rating overlay; Jellyfin browsing and
MediaFlick's progressive catalog readiness never wait on this service.

Older clients ignore the new capability data. Unsupported ratings boundary
versions receive `409` with the supported range, while a plugin without a valid
server key simply omits `ratings-v1` from the established capability list.

## Seerr request activity

Seerr reports one `processing` state for everything between an approved
request and an imported file. For every Seerr result, title detail, season,
and request in that state, the Companion adds an `activity` (`activity4k`, or
`mediaActivity` on requests) that explains it:

- `downloading` when Seerr's own download queue lists the title (or, for a
  season, one of its episodes);
- otherwise, from the configured Radarr: `searching` when the movie is
  available, `in-cinemas` or `unreleased` while Radarr waits for a digital or
  physical release;
- or from the configured Sonarr, matched by TVDB id: `unreleased` when no
  monitored episode has aired, `searching` when aired episodes are missing,
  and `awaiting-episodes` when every aired episode is present and more are
  scheduled.

Each lookup is one fixed by-id request (`/api/v3/movie?tmdbId=` or
`/api/v3/series?tvdbId=`), cached for five minutes in bounded memory, and
bounded to a three-second budget per Seerr call. A failing service is skipped
for a minute. The activity is null whenever those facts are unavailable, and a
lookup failure never fails the Seerr response. The regular-quality Radarr and
Sonarr file counts are not applied to 4K states.

Radarr and Sonarr are optional here. Without them, requests work unchanged
(Seerr hands them to its own download services), `downloading` still comes
from Seerr's queue, and every other in-progress title has a null activity,
which Desktop shows as "Processing" with a description saying the request is
approved and on its way.

## Seerr studio and network discovery

`seerr-discovery-v5` adds Discover filters by studio (movie catalog) and by
network (TV catalog):

- `GET /MediaFlick/seerr/discover/movies?studio=<TMDB company id>` and
  `GET /MediaFlick/seerr/discover/tv?network=<TMDB network id>`. Ids must be
  positive, and each parameter is rejected with `400` on any other catalog.
  Both combine with the genre, decade, sort and score filters.
- `GET /MediaFlick/seerr/companies?query=&page=` searches TMDB companies
  through Seerr as the mapped user. The trimmed query must be 1 to 100
  characters and the page 1 to 500; answers carry only company ids and names.
- `GET /MediaFlick/seerr/networks` returns the curated network list in
  `Services/SeerrNetworks.cs`. Neither Seerr nor TMDB can search networks, so
  the plugin owns this list and updates it with new releases. Each id was
  checked against `themoviedb.org/network/{id}`.
- Title details add `studioRefs` and `networkRefs` (id and name) beside the
  name-only `studios` and `networks`, which older Desktops keep reading.

## Collection experience v1

Authenticated Desktop clients use `collection-experience-v1` for normalized
TMDB Discover and exact-collection results, public MDBList search and list
results, franchise resolution, IMDb/TVDB-to-TMDB mapping, and provider artwork.
The plugin filters adult results, owns request bounds and caching, accepts only
public MDBList selectors, and reduces private, forbidden, or missing lists to
`List not available`. Desktop never receives provider credentials or calls a
provider host directly.

This contract replaces the former derived, curated, and native-mirroring
collection APIs. The hard cut does not import legacy collection definitions and
does not create, change, or delete Jellyfin BoxSets.
