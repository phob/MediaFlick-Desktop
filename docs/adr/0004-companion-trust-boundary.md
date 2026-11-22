# 0004. Provider credentials and calls live only in the server-side Companion

Status: Accepted (2026-11-22). Decided in `afddf86e` (Companion, 2026-07-29)
and `6a5007c8` (Desktop's direct Seerr client removed, 2026-08-27).

## Context

Discover, requests, ratings, release status and collections need Seerr,
Sonarr, Radarr, MDBList and TMDB. Their instance API keys grant administrator
access, and MDBList and TMDB quotas belong to one administrator's key. Desktop
runs on every user's machine and embeds a web UI.

## Decision

The optional MediaFlick Companion, a Jellyfin plugin, holds every provider
address and key, encrypted on the Jellyfin server, and makes every provider
call to a fixed origin. It exposes typed, Jellyfin-authenticated endpoints and
runs Seerr calls as the mapped Jellyfin user. Desktop reaches it with the
user's Jellyfin session and never receives a credential or a provider address;
even TMDB artwork comes through the Companion. The embedded UI talks only to
the native API in `src/shell/cef/api/`.

## Rejected alternatives

- Provider API keys in Desktop settings: every client machine would hold
  administrator access to the provider, and each client would spend the shared
  quota on its own.
- Desktop signing in to Seerr per user and keeping the session cookie, which
  Desktop did before the Companion: it still put a Seerr credential on the
  client and gave each client its own provider setup.
- A generic proxy endpoint in the Companion: it would turn the plugin into a
  way to reach arbitrary hosts with server credentials.
- Network calls from the UI: they would bypass the native API's account
  scoping and validation and expose provider hosts to page code.

## Consequences

- Provider-backed features need the Companion, and therefore Jellyfin 12.0.0.
  Everything else (sign-in, browsing, search, playback and sync) must keep
  working on Jellyfin 10.11 without it.
- A provider feature is a contract change on both sides: Companion endpoint,
  models and tests, and the Desktop client. See
  [`plugin/README.md`](../../plugin/README.md).
- Keyless public sources are outside this boundary: Desktop's native code
  fetches public Letterboxd profiles itself.
