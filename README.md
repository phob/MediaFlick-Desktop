<h1 align="center">
  <img src="resources/app-icon.png" alt="MediaFlick Desktop logo" width="180" height="180">
  <br>
  MediaFlick Desktop
</h1>

<p align="center"><b>Jellyfin playback that works immediately and still respects a serious mpv setup.</b></p>

<p align="center">
  A bundled libmpv player is the Windows default, so playback needs no separate install. Power users can switch
  to their own external <code>mpv</code> process for SVP4, custom shaders, HDR profiles, and their full
  <code>mpv.conf</code>. Watched state and resume points sync straight back to the server in either mode.
</p>

<p align="center">
  <a href="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml"><img src="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml/badge.svg" alt="Draft Release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/v/release/phob/mediaflick-desktop?display_name=tag&sort=semver" alt="Latest release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/downloads/phob/mediaflick-desktop/total" alt="Downloads"></a>
</p>

## Why this exists, and why it's different

Most people should not have to install or configure a player before watching something. MediaFlick therefore ships a focused libmpv runtime on Windows and uses it by default. It opens video in a dedicated native player window while the app keeps Jellyfin progress, resume position, and watched state synchronized.

That convenience does not replace the original power-user path. Select **External mpv** and MediaFlick hands the stream to the executable you configured. Your `mpv.conf`, scripts, shaders, SVP4 pipeline, HDR profiles, and input bindings continue to apply. Original-quality direct playback remains the default in both modes, with optional automatic or bitrate-limited Jellyfin transcoding for slower connections.

Browsing is fast because it isn't a web client talking to a server on every keystroke: the app keeps a **local SQLite mirror of your library** and searches it directly, so type-ahead search stays instant even on a big library and posters keep working while the server is slow.

The catch with playing outside the browser is usually that Jellyfin loses track of what you watched. MediaFlick solves that: while mpv plays, it reports **playback start, progress, watched state, and resume position** back to your server, so your library stays correct across every device.

## Features

- **Built-in playback on Windows** — a bundled libmpv runtime works without downloading or configuring a separate player.
- **Full external mpv mode** — hand streams to your own `mpv` process when you want `mpv.conf`, scripts, shaders, profiles, SVP4, custom HDR, or personal input bindings.
- **Selectable streaming quality** — keep original quality, use Jellyfin's automatic connection limit, or cap playback from 1.5 to 120 Mbps with server-transcoding fallback.
- **Playstate synced to Jellyfin** — playback start, progress, watched state, and resume positions report back to your server.
- **Media-segment skipping** — skip intros, credits, recaps, and commercials, with per-type prompt or auto-skip (countdown) settings.
- **Skip markers on the seek bar** — the mpv timeline shows exactly where segments are skipped, merged with the file's own chapters.
- **Its own native UI** — sign-in (password or Quick Connect), home rows for Continue Watching / Next Up, Recently Added, Latest Movies, and Latest Shows, a virtualized poster grid, and a details view with cast, seasons, and episodes.
- **Local metadata cache** — your library is mirrored into SQLite with full-text search over titles, overviews, genres, and cast, kept current by a background sync.
- **Release calendar** — agenda and month views of upcoming episodes and film releases. It works from Jellyfin metadata alone and gains monitored/file truth from the optional Companion plugin.
- **Server-mediated requests and ratings** — the optional MediaFlick Companion keeps Sonarr, Radarr, Seerr, MDBList, and TMDB credentials on the Jellyfin server. It serves MDBList ratings through a quota-aware shared cache without exposing the administrator key to Desktop.
- **Two collection modes** — use exact TMDB franchises and template-created personal collections, or browse Jellyfin BoxSets unchanged. Collection preferences stay local to each account.
- **Server administration in your browser** — anything the app deliberately doesn't rebuild (dashboard, users, metadata editing) opens in your default browser from the right-click menu.
- **Optional external-player setup** — one-click mpv download on Windows; Linux and macOS auto-detect a system `mpv`.
- **Optional MPC-HC backend on Windows** — switchable live from Client Settings.
- **Automatic in-app updates** from GitHub Releases (Windows).
- **Cross-platform** — Windows, Linux, and macOS.

## Install

**Windows** — download the latest `MediaFlickDesktop-Setup-<version>.exe` from [Releases](https://github.com/phob/mediaflick-desktop/releases/latest) and run it. The built-in player is ready immediately; installing external mpv is optional.

On first launch, enter your Jellyfin server address and sign in. Right-click anywhere for **Client Settings**, **Open Jellyfin dashboard**, and **About**.

**Linux / macOS** — grab the release archive and run `mediaflick-desktop`. mpv isn't bundled; a system `mpv` is auto-detected.

## Build it yourself

See [BUILDING.md](BUILDING.md).

## MediaFlick Companion

The optional server plugin lives in [`plugin/`](plugin/README.md). It targets
Jellyfin 10.11.11 and exposes only typed, authenticated operations—there is no
generic service proxy. Configure Sonarr, Radarr, Seerr, MDBList, and TMDB support
from Jellyfin's plugin dashboard. Desktop clients discover it automatically,
report plugin and service availability under **Settings → MediaFlick Companion**,
and never receive service addresses or API keys. MDBList ratings use a
shared stable-ID cache, stale-while-revalidate, bounded batches, and durable
quota/backoff state so one administrator key can safely serve multiple users.

## Local data ownership

MediaFlick keeps device settings in `settings.json` and account-owned intent in
`accounts.json`, `collections.json`, and `playback-preferences.json`. Custom
collection posters live beside those app-owned files. They are not a supported
manual-editing interface; use Settings so writes, backups, and validation stay
atomic. `library.db` contains only rebuildable catalog and collection results
and may be recreated without removing those preferences or posters.
