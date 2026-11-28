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

MediaFlick runs mpv in two modes and reports playback start, progress, watched state, and resume points back to Jellyfin in both:

- **Built-in player (default on Windows and Linux).** Windows releases and Linux AppImages bundle a focused `libmpv` runtime, so nothing else needs to be installed. Video renders natively inside the app window under MediaFlick's own controls. Windows auto-detects SVP 4. Linux renders over X11 or XWayland via Vulkan with an OpenGL fallback; unpackaged builds can also load system `libmpv.so.2`. See [Linux setup](BUILDING.md#build-linux-and-macos-release-packages).
- **External mpv.** Point MediaFlick at your own `mpv` executable to keep your `mpv.conf`, GLSL shaders, Lua scripts, HDR profiles, SVP 4 setup, and `input.conf` bindings intact. Windows offers a one-click mpv download in Settings. Linux and macOS ([untested alpha](#macos-untested-alpha)) auto-detect a system `mpv`.

Both modes default to original-quality direct playback. You can also choose Jellyfin's automatic quality limit or cap streams from 1.5 to 120 Mbps with server transcoding fallback. MediaFlick also registers as a Jellyfin cast target, so **Play On** from other Jellyfin apps can start and control movies, episodes, music, and audiobooks.

**Segment skipping** can be configured independently for intros, credits, recaps, and commercials: **Always skip** with a countdown, **Ask me**, **Never**, or **Don't show**. **Never** still shows the Skip button and only turns off automatic skipping. **Don't show** hides the prompt, Skip button, and timeline markers altogether. Active markers appear on the seek bar alongside the file's own chapters.

## Library, browsing & media

Browsing does not wait on Jellyfin for every keystroke. MediaFlick mirrors your catalog into a local SQLite database kept current by live server updates and background sync. Type-ahead search runs locally across titles, original titles, and genres, and cached posters stay responsive even when the server is slow.

- **Home & libraries.** Sign in with a password or Quick Connect. Home pairs a rotating billboard with Continue Watching / Next Up, Recently Added and Latest rows for Movies and Shows, per-library shelves, Favorites, Releases, and custom collection shelves. Libraries use virtualized poster grids with Jellyfin-style sorting, remembered filters, hover previews, and an A–Z jump rail. Title pages show cast, crew, seasons, episodes, and full stream details.
- **Releases.** Agenda and month views track upcoming episodes and films, backed by a Home **Releases** shelf centered on today. It works from Jellyfin metadata alone. With the [Companion](#mediaflick-companion), Sonarr and Radarr add monitored and on-disk status, and the Home shelf leads with the past 30 days marked downloaded or missing.
- **Collections.** Browse exact TMDB movie franchises split into owned and missing films, and build personal collections from templates or public MDBList lists with your own posters. If you prefer your existing Jellyfin BoxSets, switch to Jellyfin mode to view them untouched.
- **Letterboxd.** Link public Letterboxd profiles to display friends' star ratings and reviews on movie pages. No Letterboxd account or API key is required.
- **Music & audiobooks.** Play albums, tracks, and audiobooks through the built-in player or External mpv with a now-playing bar, queue, shuffle, and media keys while you keep browsing.
- **Built-in book reader.** Open books in a sandboxed full-window reader with scripts disabled. Supports EPUB, MOBI, AZW, AZW3, PDF, CBZ, CBR (RAR 4 and 5), CB7, and CBT. Navigate by keyboard, TV remote, or mouse; exact reading progress is saved per account and synced to Jellyfin.
- **Server admin in your browser.** Anything MediaFlick deliberately leaves to Jellyfin—the server dashboard, users, and metadata editing—opens in your default browser via right-click → **Open Jellyfin dashboard**.

## MediaFlick Companion

The optional server plugin lives in [`plugin/`](plugin/README.md) and runs on **Jellyfin 12.0.0 and later 12.x releases**. On Jellyfin 10.11, Desktop works fully without it, and **Settings → MediaFlick Companion** notes that the plugin requires Jellyfin 12.0.0.

Provider URLs and API keys stay encrypted on the Jellyfin server. Desktop never sees credentials and there is no generic proxy—only typed, Jellyfin-authenticated endpoints so one administrator setup serves every client:

- **Discover & requests (Seerr, Sonarr, Radarr).** Browse and filter by genre, decade, score, studio, or network, and request films, individual seasons, or 4K. With Sonarr and Radarr connected, Seerr's generic "Processing" label resolves into concrete states: Downloading, Searching, In cinemas, Unreleased, or Awaiting episodes.
- **Shared ratings cache (MDBList).** Show scores from IMDb, Rotten Tomatoes, Metacritic, Letterboxd, and other sources on cards and detail pages. One server-wide, quota-aware cache uses stale-while-revalidate, batched lookups, and durable rate-limit backoff so a single admin key serves all users safely.
- **Franchises & template collections (TMDB, MDBList).** Resolve exact TMDB franchises with Request buttons on missing titles, plus auto-updating personal collections.

Install the plugin from its Jellyfin repository by following [flick.media/companion](https://flick.media/companion/), then configure services in Jellyfin's plugin dashboard. Desktop detects it automatically and shows service availability under **Settings → MediaFlick Companion**.

## Settings & shortcuts

Settings are divided into **Client** (this machine, available before sign-in), **Account** (per signed-in user), and **Integrations** (MediaFlick Companion and Letterboxd). Every settings page edits a draft with **Save**, **Discard**, and **Restore defaults** (**Undo changes** on Collections), and prompts before leaving unsaved changes.

**Account (Viewing, Home, Appearance, Collections):**

- **Spoiler protection & up-next.** Viewing hides unwatched episode thumbnails, names, and summaries behind a Reveal button, and conceals upcoming calendar episode titles conservatively. Next-episode playback can be off, a cancellable countdown, or automatic, with an optional episode limit for continuous playback (starting a title manually begins a new session, and explicit Next stays available).
- **Languages & resume.** Set ordered audio and subtitle language codes, prefer tracks explicitly labeled original, choose subtitle rules, and pick a 0, 5, 10, or 30-second resume rewind. Valid per-title track choices take priority.
- **Browsing & appearance.** Viewing sets text size, preset poster widths, startup page (Home, any library, Releases, or last browsing page), and remembered per-library filters (overridden by explicit filtered links). Appearance controls accent color, density, artwork and backdrop intensity, reduced motion, rating sources, and **Cards** hover previews with a live-previewed hover delay.

**Client (Player, Playback, Application):**

- **Built-in player controls.** Player sets subtitle size, outline, background, and vertical position with a live preview. Subtitle appearance takes effect on the next playback, though bitmap or authored styles may limit it. You can also tune seek intervals and record conflict-checked shortcuts for pause, mute, fullscreen, seeking, stop, subtitle visibility, and mark-watched-and-play-next (`W` by default). Clearing a shortcut disables it; `Space` always pauses, and `Left` and `Right` always use your saved seek intervals.
- **External mpv controls.** External mpv keeps its own subtitle styling and native key bindings. Settings shows only the mark-watched-and-play-next binding, which marks a movie watched or marks an episode watched and starts the next available item.
- **Playback & Application.** Playback holds default streaming quality and per-type segment-skipping rules. Application controls window close behavior, scrollbars, and log level.

## Install

**Jellyfin server.** MediaFlick Desktop requires **Jellyfin 10.11.0 or newer** and tells you on the sign-in screen if a server is too old before sending credentials. The optional [MediaFlick Companion](#mediaflick-companion) requires **Jellyfin 12.0.0 or newer**; on Jellyfin 10.11 the app works fully without it.

**Windows.** Download `MediaFlickDesktop-Setup-<version>.exe` or the portable `MediaFlickDesktop-v<version>-windows-x64.zip` from [Releases](https://github.com/phob/mediaflick-desktop/releases/latest). The built-in player works immediately; external mpv is optional. Installed copies update automatically from GitHub Releases, while portable copies update by replacing the folder with the next zip.

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
