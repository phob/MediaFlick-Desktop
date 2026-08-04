<h1 align="center">
  <img src="resources/app-icon.png" alt="MediaFlick Desktop logo" width="180" height="180">
  <br>
  MediaFlick Desktop
</h1>

<p align="center"><b>The Jellyfin desktop client that plays through your <i>real</i> mpv.</b></p>

<p align="center">
  Not libmpv baked into an app — your actual external <code>mpv</code> process, with SVP4 motion
  interpolation, SDR-to-HDR, custom shaders, and your full <code>mpv.conf</code>. Browse a fast native
  library, hit play, and mpv takes over while watched state and resume points sync straight back to your server.
</p>

<p align="center">
  <a href="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml"><img src="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml/badge.svg" alt="Draft Release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/v/release/phob/mediaflick-desktop?display_name=tag&sort=semver" alt="Latest release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/downloads/phob/mediaflick-desktop/total" alt="Downloads"></a>
</p>

## Why this exists, and why it's different

Every other Jellyfin desktop app plays video *inside* itself with libmpv. That's convenient, but it quietly caps what mpv can do — the features power users actually chase, like **SVP 4 frame interpolation** and **SDR-to-HDR tone mapping**, are exactly the ones that don't survive being embedded.

MediaFlick Desktop takes the opposite approach. It has its **own UI** — no embedded Jellyfin Web — and when you press play it hands the stream to the **external `mpv` you already configured**. Original-quality direct playback remains the default, with optional automatic or bitrate-limited Jellyfin transcoding for slower connections. Your `mpv.conf`, your scripts, your shaders, your SVP4 pipeline, your HDR profiles, your input bindings — all of it applies, because it's the real mpv, not a stripped-down copy.

Browsing is fast because it isn't a web client talking to a server on every keystroke: the app keeps a **local SQLite mirror of your library** and searches it directly, so type-ahead search stays instant even on a big library and posters keep working while the server is slow.

The catch with playing outside the browser is usually that Jellyfin loses track of what you watched. MediaFlick solves that: while mpv plays, it reports **playback start, progress, watched state, and resume position** back to your server, so your library stays correct across every device.

## Features

- **Real external mpv playback** — original or server-transcoded streams handed to your own `mpv` process, so your entire setup applies: `mpv.conf`, scripts, shaders, profiles, SVP4, custom HDR, input bindings.
- **Selectable streaming quality** — keep original quality, use Jellyfin's automatic connection limit, or cap playback from 1.5 to 120 Mbps with server-transcoding fallback.
- **Playstate synced to Jellyfin** — playback start, progress, watched state, and resume positions report back to your server.
- **Media-segment skipping** — skip intros, credits, recaps, and commercials, with per-type prompt or auto-skip (countdown) settings.
- **Skip markers on the seek bar** — the mpv timeline shows exactly where segments are skipped, merged with the file's own chapters.
- **Its own native UI** — sign-in (password or Quick Connect), home rows for Continue Watching / Next Up / Recently Added, a virtualized poster grid, and a details view with cast, seasons, and episodes.
- **Local metadata cache** — your library is mirrored into SQLite with full-text search over titles, overviews, genres, and cast, kept current by a background sync.
- **Release calendar** — agenda and month views of upcoming episodes and film releases. It works from Jellyfin metadata alone and gains monitored/file truth from the optional Companion plugin.
- **Server-mediated requests and ratings** — the optional MediaFlick Companion keeps Sonarr, Radarr, Seerr, MDBList, and preparation-only TMDB credentials on the Jellyfin server. A local Desktop MDBList key wins; the plugin is a quota-aware cached fallback only when no valid local key exists.
- **Server administration in your browser** — anything the app deliberately doesn't rebuild (dashboard, users, metadata editing) opens in your default browser from the right-click menu.
- **One-click mpv download on Windows** — no manual setup; Linux and macOS auto-detect a system `mpv`.
- **Optional MPC-HC backend on Windows** — switchable live from Client Settings (mpv stays the default).
- **Automatic in-app updates** from GitHub Releases (Windows).
- **Cross-platform** — Windows, Linux, and macOS.

## Install

**Windows** — download the latest `MediaFlickDesktop-Setup-<version>.exe` from [Releases](https://github.com/phob/mediaflick-desktop/releases/latest) and run it. Don't have mpv yet? There's a one-click **Download mpv** in the app.

On first launch, enter your Jellyfin server address and sign in. Right-click anywhere for **Client Settings**, **Open Jellyfin dashboard**, and **About**.

**Linux / macOS** — grab the release archive and run `mediaflick-desktop`. mpv isn't bundled; a system `mpv` is auto-detected.

## Build it yourself

See [BUILDING.md](BUILDING.md).

## MediaFlick Companion

The optional server plugin lives in [`plugin/`](plugin/README.md). It targets
Jellyfin 10.11.11 and exposes only typed, authenticated operations—there is no
generic service proxy. Configure Sonarr, Radarr, Seerr, MDBList, and preparation-only TMDB support
from Jellyfin's plugin dashboard; desktop clients discover it automatically
through Jellyfin and never receive the service API keys. MDBList ratings use a
shared stable-ID cache, stale-while-revalidate, bounded batches, and durable
quota/backoff state so one administrator key can safely serve multiple users.
