<h1 align="center">
  <img src="resources/app-icon.png" alt="MediaFlick Desktop logo" width="180" height="180">
  <br>
  MediaFlick Desktop
</h1>

<p align="center"><b>The Jellyfin desktop client that plays through your <i>real</i> mpv.</b></p>

<p align="center">
  Not libmpv baked into an app — your actual external <code>mpv</code> process, with SVP4 motion
  interpolation, SDR-to-HDR, custom shaders, and your full <code>mpv.conf</code>. Browse in Jellyfin
  Web, hit play, and mpv takes over while watched state and resume points sync straight back to your server.
</p>

<p align="center">
  <a href="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml"><img src="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml/badge.svg" alt="Draft Release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/v/release/phob/mediaflick-desktop?display_name=tag&sort=semver" alt="Latest release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/downloads/phob/mediaflick-desktop/total" alt="Downloads"></a>
</p>

<!-- DEMO: 15–20s loop — click a poster in the grid → mpv window launches → skip-intro prompt appears on the seek bar. -->
<p align="center">
  <img src="docs/demo.gif" alt="Clicking a poster in the library grid hands the stream to an external mpv window with skip-segment markers on the seek bar" width="820">
</p>

## Why this exists, and why it's different

Every other Jellyfin desktop app plays video *inside* itself with libmpv. That's convenient, but it quietly caps what mpv can do — the features power users actually chase, like **SVP 4 frame interpolation** and **SDR-to-HDR tone mapping**, are exactly the ones that don't survive being embedded.

MediaFlick Desktop takes the opposite approach. It shows you Jellyfin Web in a native window, but when you press play it hands the stream to the **external `mpv` you already configured**. Original-quality direct playback remains the default, with optional automatic or bitrate-limited Jellyfin transcoding for slower connections. Your `mpv.conf`, your scripts, your shaders, your SVP4 pipeline, your HDR profiles, your input bindings — all of it applies, because it's the real mpv, not a stripped-down copy.

The catch with playing outside the browser is usually that Jellyfin loses track of what you watched. MediaFlick solves that: while mpv plays, it reports **playback start, progress, watched state, and resume position** back to your server, so your library stays correct across every device.

### See the difference

<!-- DEMO: SVP4 / SDR-to-HDR before-after. Side-by-side or slider — same frame, mpv-embedded client vs MediaFlick's external mpv with SVP4 + HDR active. -->
<p align="center">
  <img src="docs/svp-hdr-demo.gif" alt="SVP4 motion interpolation and SDR-to-HDR running through MediaFlick's external mpv" width="820">
</p>

## Features

- **Real external mpv playback** — original or server-transcoded streams handed to your own `mpv` process, so your entire setup applies: `mpv.conf`, scripts, shaders, profiles, SVP4, custom HDR, input bindings.
- **Selectable streaming quality** — keep original quality, use Jellyfin's automatic connection limit, or cap playback from 1.5 to 120 Mbps with server-transcoding fallback.
- **Playstate synced to Jellyfin** — playback start, progress, watched state, and resume positions report back to your server.
- **Media-segment skipping** — skip intros, credits, recaps, and commercials, with per-type prompt or auto-skip (countdown) settings.
- **Skip markers on the seek bar** — the mpv timeline shows exactly where segments are skipped, merged with the file's own chapters.
- **Jellyfin Web in a native window** — rendered in a CEF shell, with infinite scroll on the poster grid instead of pagination.
- **One-click mpv download on Windows** — no manual setup; Linux and macOS auto-detect a system `mpv`.
- **Optional MPC-HC backend on Windows** — switchable live from Client Settings (mpv stays the default).
- **Automatic in-app updates** from GitHub Releases (Windows).
- **Cross-platform** — Windows, Linux, and macOS.

## Install

**Windows** — download the latest `MediaFlickDesktop-Setup-<version>.exe` from [Releases](https://github.com/phob/mediaflick-desktop/releases/latest) and run it. Don't have mpv yet? There's a one-click **Download mpv** in the app.

**Linux / macOS** — grab the release archive and run `mediaflick-desktop`. mpv isn't bundled; a system `mpv` is auto-detected.

## Build it yourself

See [BUILDING.md](BUILDING.md).

## A note on AI assistance

This is a personal project I built to scratch my own itch. The Rust and JavaScript that make up the app are written and reviewed by hand; AI tooling helped with the cross-platform Linux/macOS builds, the GitHub Actions release automation, and code review. Mentioning it because I'd rather be upfront than have you wonder.
