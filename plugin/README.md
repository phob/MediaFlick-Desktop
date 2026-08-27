# MediaFlick Companion

The server-side companion for MediaFlick Desktop. It exposes typed,
Jellyfin-authenticated endpoints under `/MediaFlick`, keeps provider
credentials on the server, and runs Seerr calls as the mapped Jellyfin user.
There is no arbitrary proxy endpoint.

The plugin targets Jellyfin 10.11.11 (`net9.0`). Build it with `just plugin`,
test it with `just plugin-test`, and deploy the release publish output to the
configured development server with `just plugin-deploy`.

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
metadata. Missing ratings are omitted without failing the batch. Unknown future
MDBList sources survive under a bounded safe identifier.

## Shared quota and cache behavior

The administrator's MDBList quota is shared by every Jellyfin user (the
published free tier is 1,000 requests/day), so the plugin maintains one
server-wide, stable-ID cache—not a cache per client or card. Entries are fresh
for seven days (negative results for one day), remain
stale-servable for 30 days, and are persisted atomically in the plugin data
directory. Stale entries return immediately while one deduplicated background
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
