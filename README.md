<h1 align="center">
  <img src="distribution/app-icon.png" alt="MediaFlick Desktop logo" width="180" height="180">
  <br>
  MediaFlick Desktop
</h1>

<p align="center"><b>A native Jellyfin desktop client built around mpv and a local library index.</b></p>

<p align="center">
  Plays immediately with bundled libmpv on Windows and Linux, or hands streams off to your own configured
  <code>mpv</code> with shaders, scripts, HDR profiles, and SVP 4 intact. Search runs locally against SQLite,
  watched state syncs back to Jellyfin, and an optional server plugin wires in Sonarr, Radarr, Seerr, MDBList, and TMDB.
</p>

<p align="center">
  <a href="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml"><img src="https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml/badge.svg" alt="Build Release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/v/release/phob/mediaflick-desktop?display_name=tag&sort=semver" alt="Latest release"></a>
  <a href="https://github.com/phob/mediaflick-desktop/releases/latest"><img src="https://img.shields.io/github/downloads/phob/mediaflick-desktop/total" alt="Downloads"></a>
</p>

## Playback

MediaFlick uses mpv in two modes, and reports playback start, progress, watched state, and resume points back to Jellyfin in both:

- **Built-in player (default on Windows and Linux).** Windows releases and Linux AppImages bundle a focused `libmpv` runtime that renders native video directly inside the app window under MediaFlick's own controls. No separate player install is needed. On Windows it automatically detects SVP 4; on Linux it renders via Vulkan with an OpenGL fallback over X11 or XWayland (unpackaged builds can load system `libmpv.so.2`; see [Linux setup](docs/libmpv-integration.md#integrated-linux-rendering)).
- **External mpv.** Point MediaFlick at your own `mpv` binary to keep your full `mpv.conf`, GLSL shaders, Lua scripts, HDR profiles, SVP 4 pipeline, and `input.conf` bindings untouched. Windows has a one-click mpv download in Settings; Linux and macOS ([untested alpha](#macos-untested-alpha)) auto-detect a system `mpv`.

Both modes default to original-quality direct playback, with optional automatic connection limits or fixed bitrate caps from 1.5 to 120 Mbps (falling back to Jellyfin server transcoding when needed). MediaFlick also acts as a Jellyfin cast target, so **Play On** from other Jellyfin apps can start and control movies, episodes, music, and audiobooks.

**Segment skipping** handles intros, credits, recaps, and commercials per type: **Always skip** (with a countdown), **Ask me**, **Never** (keeps the Skip button visible without auto-skipping), or **Don't show** (removes the prompt, Skip button, and timeline markers entirely). Active segment markers are merged into the seek bar alongside the file's own chapters.

## Library, browsing & media

Browsing does not wait on round trips to Jellyfin for every keystroke. MediaFlick mirrors your catalog into a local SQLite database kept current by live server updates and background sync, so type-ahead search across titles, overviews, genres, and cast answers immediately and cached posters stay responsive even when the server is busy.

- **Native interface.** Sign in with a password or Quick Connect. Home includes a rotating billboard, Continue Watching / Next Up, Recently Added Movies & Shows, Latest Movies & Shows, per-library shelves, Favorites, Releases, and custom collection shelves. Library views use virtualized poster grids with Jellyfin-style sorting, remembered filters, hover previews, and an A–Z jump rail; title pages show cast, crew, seasons, episodes, and full stream/track details.
- **Releases.** Agenda and month views for upcoming episodes and films, plus a Home **Releases** shelf centered on today. Works from Jellyfin metadata alone; with the [Companion](#mediaflick-companion), it pulls monitored/on-disk truth from Sonarr and Radarr and leads the Home shelf with the past 30 days marked downloaded or missing.
- **Collections.** Use exact TMDB movie franchises (split into owned and missing films) alongside personal collections built from templates or public MDBList lists with custom posters, or stick to your existing Jellyfin BoxSets unchanged.
- **Letterboxd.** Add public Letterboxd profiles (no account or API key needed) to show friends' stars and reviews on movie pages.
- **Music, audiobooks & books.** Albums, tracks, and audiobooks play through the built-in player or External mpv with a now-playing bar, queue, shuffle, and media keys while you browse. Books open in a sandboxed full-window reader supporting EPUB, MOBI, AZW, AZW3, PDF, CBZ, CBR (RAR 4 and 5), CB7, and CBT, controlled by keyboard, TV remote, or mouse, with exact reading progress saved per account and synced to Jellyfin.
- **Server admin in your browser.** Anything MediaFlick deliberately leaves to Jellyfin (dashboard, users, metadata editing) opens in your default browser via right-click → **Open Jellyfin dashboard**.

## MediaFlick Companion

The optional server plugin lives in [`plugin/`](plugin/README.md) and runs on **Jellyfin 12.0.0 and later 12.x releases**. On Jellyfin 10.11, Desktop works fully without it; **Settings → MediaFlick Companion** notes when a server is below 12.0.0.

MediaFlick Desktop never holds provider URLs or API keys, and the plugin does not expose a generic proxy. Everything goes through typed, Jellyfin-authenticated endpoints so one administrator setup serves every client:

- **Discover & requests (Seerr, Sonarr, Radarr).** Browse, filter (genre, decade, score, studio, network), and request movies, individual seasons, or 4K. With Sonarr and Radarr configured, Seerr's generic "Processing" status resolves to concrete states (Downloading, Searching, In cinemas, Unreleased, Awaiting episodes).
- **Shared ratings cache (MDBList).** Displays review scores (IMDb, Rotten Tomatoes, Metacritic, Letterboxd, and more) across cards and details through a single server-wide, quota-aware cache with stale-while-revalidate, batched lookups, and durable rate-limit backoff.
- **Franchises & template collections (TMDB, MDBList).** Resolves exact TMDB franchises with request buttons on missing entries, plus auto-updating personal collections.

Install the plugin from its Jellyfin plugin repository using the guide at [flick.media/companion](https://flick.media/companion/), then configure services in Jellyfin's plugin dashboard. Desktop discovers it automatically and reports service status under **Settings → MediaFlick Companion**.

## Settings & shortcuts

Settings are split into **Account** preferences (per signed-in user) and **Client** preferences (local to this machine and available before sign-in). Every page edits a draft with explicit **Save**, **Restore defaults**, and **Discard** buttons and prompts before leaving unsaved changes.

**Account (Viewing, Home, Appearance, Collections):**

- **Viewing** covers spoiler protection (hiding unwatched episode thumbnails, names, and summaries behind a Reveal button, plus conservative hiding on upcoming calendar entries), next-episode behavior (off, cancellable countdown, or automatic), and continuous-playback episode limits (manual starts begin a new session and explicit Next stays available).
- Audio and subtitle defaults use ordered language codes, an original-audio option (for tracks explicitly labeled original), subtitle rules, and a 0/5/10/30-second resume rewind. Valid per-title track choices always take priority.
- Layout and navigation options set text size, preset poster widths, startup destination (Home, any library, Releases, or last browsing page), and remembered per-library filters (overridden by explicit filtered links).
- **Appearance** sets accent color, density, artwork and backdrop intensity, reduced motion, rating sources, and **Cards** hover previews with configurable hover delay (disabled when previews are off; draft timing previews live).

**Client (Player, Playback, Application):**

- **Player** chooses between the built-in player and External mpv. For the built-in player, it includes subtitle size, outline, background, and vertical position with a live preview (applied on the next playback; bitmap or authored subtitle styles may limit the effect), custom seek intervals, and a conflict-checked shortcut recorder for pause, mute, fullscreen, seeking, stop, subtitle visibility, and mark-watched-and-play-next (`W` by default). Clearing a binding disables it; `Space` always pauses and `Left`/`Right` always use your saved seek intervals.
- With **External mpv**, mpv keeps its own subtitle styling and native bindings; Settings shows only the mark-watched-and-play-next shortcut (marks a movie watched, or marks an episode watched and starts the next available item).
- **Playback** holds default streaming quality and per-type segment-skipping rules; **Application** controls window close behavior, scrollbars, and log level.

## Install

**Jellyfin server.** MediaFlick Desktop requires **Jellyfin 10.11.0 or newer** and tells you on the sign-in screen if a server is too old before sending credentials. The optional [MediaFlick Companion](#mediaflick-companion) requires **Jellyfin 12.0.0 or newer**; on Jellyfin 10.11 the app works fully without it.

**Windows.** Download `MediaFlickDesktop-Setup-<version>.exe` (or the portable `MediaFlickDesktop-v<version>-windows-x64.zip`) from [Releases](https://github.com/phob/mediaflick-desktop/releases/latest). The built-in player works immediately; installing external mpv is optional. Installed copies update automatically from GitHub Releases (portable copies update by replacing the folder with the next zip).

**Linux.** Download `MediaFlickDesktop-v<version>-linux-x86_64.AppImage` from [Releases](https://github.com/phob/mediaflick-desktop/releases/latest), make it executable, and run it. The AppImage bundles its own `libmpv` runtime and uses X11 or XWayland. If you switch to External mpv, MediaFlick auto-detects your system `mpv`.

On first launch, enter your Jellyfin server address and sign in. Right-click anywhere in the app for **Fullscreen**, **Settings**, **Open Jellyfin dashboard**, and **About**.

### macOS (untested alpha)

macOS support is an **untested alpha**. Every macOS build through 0.2.0 crashed at startup. Builds now start, but have only been run on GitHub-hosted Apple silicon VMs without a GPU, where sign-in, browsing, detail pages, and playback in external mpv with software video passed.

macOS requires the first release after 0.2.0; 0.2.0 and earlier crash at startup. Download `MediaFlickDesktop-v<version>-macos-arm64.dmg` (Apple silicon) or `MediaFlickDesktop-v<version>-macos-x86_64.dmg` (Intel) from [Releases](https://github.com/phob/mediaflick-desktop/releases). Playback requires external mpv (for example, `brew install mpv`); the built-in player is not available on macOS.

Known limitations and untested areas:

- Builds are ad-hoc signed and not notarized, so Gatekeeper blocks the first launch. The standard macOS workaround (not yet observed on a real Mac): after the blocked launch, open **System Settings → Privacy & Security** and choose **Open Anyway** (Control-click → Open no longer bypasses it on macOS 15 and later), or run `xattr -dr com.apple.quarantine "/Applications/MediaFlick Desktop.app"`.
- Playback on a real Mac with a GPU is unproven: mpv's default video output crashed on the test VMs' virtual GPU.
- Intel (`x86_64`) builds have never been run.
- Audio, the update notice, and updating over an earlier install are untested.
- Keychain-backed cookies and staying signed in across restarts are untested.

## Local data

Device settings live in `settings.json`, while per-account state lives in `accounts.json`, `collections.json`, `playback-preferences.json`, and `reader.json`, with custom collection posters stored beside them. These files are not a manual-editing interface; change them through Settings so writes, validation, and backups stay atomic.

`library.db` stores the active Jellyfin session alongside rebuildable catalog and collection caches. Recreating the database keeps the account signed in and leaves preferences, reading progress, and custom posters untouched.

## Build it yourself

See [BUILDING.md](BUILDING.md) for desktop builds and packaging, and [`plugin/README.md`](plugin/README.md) for the Companion plugin.

## License

MediaFlick Desktop is free software under the [GNU General Public License v2.0 or later](LICENSE). Third-party components keep their own licenses; release attribution and bundled-runtime details are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
