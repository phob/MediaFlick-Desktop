# Changelog

## [Unreleased]

### Breaking Changes

- Replaced the embedded Jellyfin Web client with MediaFlick Desktop's own UI. The app now signs in to Jellyfin and loads its login, home, library, and details views from `mediaflick-desktop://app/`. Existing users keep their server URL but must sign in again. The "Open Jellyfin dashboard" context-menu item opens server administration in the system browser.
- Removed the jellyfin-web injection bridge, including `bridge.js` and stream-URL interception. Native code now negotiates playback through `PlaybackInfo` and sends it directly to mpv or MPC-HC. Scripts and workflows that used the injected `window.__mediaFlickDesktop*` hooks no longer work. The native About and update dialogs still use the shell bridge.
- Removed the welcome and setup screen. Users now enter the server address on the sign-in screen and configure the media player in Settings.

### Added

- Added TMDB movie collections as a library category. The Companion plugin derives the collection set from the movies the Jellyfin library already contains, resolving each movie's TMDB collection through Seerr with a bounded, concurrent, disk-persisted mapping cache that survives server restarts. The Collections page lists every collection in the library sorted by name ignoring a leading "The", "A", or "An"; a collection page shows its parts in release order with owned and missing titles distinguished, and missing titles carry the usual discover-and-request flow. Movie details link back to their collection when one exists. The category appears only when the plugin advertises `collections-v1`.
- Added the anti-slop Oxlint plugin and enabled every rule for the React UI. Updated project-owned UI code and tests to pass without blanket rule exemptions.
- Added pinned Rust checks with strict Clippy readability and ownership rules, rust-analyzer and Bacon diagnostics, a repository Codex skill, and a Rust-aware stop hook. Updated the project's Rust code and tests to pass without suppressions.
- Added a Rust file-size check. It reports source files above 1,000 physical lines and rejects those above 1,500, while ignoring build output and other Git-ignored files.
- Added Latest Movies and Latest Shows after Recently Added on the home page. Each shelf sorts its catalog by release year and links to the matching newest-first library view.
- Added an in-app `/settings/*` area. Client and Appearance are available without an account, while Letterboxd and Seerr settings belong to the signed-in account. The sidebar links to Settings, forms keep local drafts, and fields support Save, Discard, and Reset. Typed PATCH endpoints share preferences with CEF, apply player, segment-skipping, and scrollbar changes at runtime, and return normalized settings with platform capabilities.
- Added Appearance settings for system, dark, and light modes; signal, cobalt, amber, and violet accents; compact density; artwork and backdrop intensity; and reduced motion.
- Added an Appearance setting to disable pop-out card previews. When disabled, Play, My List, and watched buttons appear over Movie, Series, and Episode cards. The buttons reuse the bottom scrim without hiding the progress line or card frame. Only the buttons capture clicks, so the rest of the card still opens details.
- Added account-specific Letterboxd profile connections in SQLite. They accept a username or canonical URL, normalize it to `letterboxd.com`, and verify it with a bounded RSS request. Users can refresh, disable, open, or remove a profile.
- Added the latest ratings and written reviews from connected Letterboxd members to matching movie details. MediaFlick matches public RSS entries by TMDB movie id, converts review HTML to bounded plain text, and caches feeds for 30 minutes. A stale feed remains available after refresh failure. Slow profile requests do not delay the local detail response.
- Added connected Letterboxd ratings and reviews to Seerr movie details. They match by TMDB movie id and appear before the user requests a title.
- Added the optional MediaFlick Companion plugin for Jellyfin 10.11.11. Its authenticated, versioned `/MediaFlick` API keeps Sonarr, Radarr, and Seerr keys on the server. The admin page shows redacted keys and runs connection tests. Companion refreshes the Sonarr and Radarr calendar every 15 minutes while retaining stale data after failure. It makes Seerr calls as the mapped Jellyfin user and can import that user on first use with explicit approval. The plugin has separate build, test, deployment, CI, and draft-release jobs.
- Added Companion administration for MDBList and TMDB, plus the Desktop `ratings-v1` fallback. Data Protection encrypts administrator secrets, and API responses never return them. Capability and status responses require authentication and redact sensitive values. MDBList uses bounded stable-id batches, a shared persistent cache, duplicate-request suppression, and restart-safe quota or `Retry-After` backoff. TMDB configuration is preparatory only. Desktop credentials still take priority over Companion.
- Added Releases with agenda and month views, release-type filters, stale-data warnings, local Play or Open actions, and film requests. It uses the Companion calendar when available. Without Companion, it shows Jellyfin's metadata-only Upcoming feed.
- Added Jellyfin username and password sign-in plus Quick Connect. MediaFlick keeps a device id for each installation, lists it in Jellyfin's Devices page, restores sessions across restarts, and returns to sign-in when Jellyfin rejects a stored token.
- Added the local SQLite catalog `library.db` for movies, series, seasons, and episodes. FTS5 indexes titles and genres, while dedicated columns index TMDB, IMDb, and TVDB ids. A background thread runs a resumable initial sync, an incremental `DateCreated` sweep, and an hourly identity sweep that updates watch state and removes deleted items.
- Added MediaFlick's UI under `mediaflick-desktop://app/`. It includes sign-in, home rows for Continue Watching, Next Up, and Recently Added, plus a virtualized library grid with instant search, sorting, genre filters, and watched filters. Details show cast, seasons, and episodes, and support watched and favorite updates.
- Added a home-page billboard with Play, Resume, progress, Details, and My List actions. The home page also has a My List shelf, paged rows for mouse and keyboard use, remaining-time labels, browse-all links, and an empty state.
- Added native playback negotiation through `PlaybackInfo`. MediaFlick applies the configured streaming quality, selects a media source, builds a direct-stream or transcoding URL, and sends it to the playback coordinator. Episodes advance when mpv reports end of file or the user chooses mark watched and next.
- Added a streaming-quality picker beside Play on details. It overrides the Client setting for the rest of the session, so the user can lower one title's bitrate without changing the saved default.
- Added an in-app player bar with position, pause, resume, stop, and seek controls through `/api/player/*`. The image proxy now has a pruned disk cache for posters and keeps the access token out of the DOM.
- Added `--library-stats` and `--library-sync-once` to inspect the cache without starting the browser shell.
- Added `scripts/cdp-eval.ps1` to run JavaScript in an app window started with `--remote-debugging-port`.
- Replaced the top header with a collapsible sidebar for search, Home, Movies, Series, and Favorites. The signed-in user and server stay at the bottom with library sync and sign-out actions. The toggle or Ctrl+B collapses the sidebar to an icon rail, and the app remembers the choice across restarts.
- Added the React toolchain under `ui/` with Vite, TypeScript, Tailwind CSS v4, shadcn/ui on `radix-ui`, TanStack Query, React Router, and `@tanstack/react-virtual`. Tailwind uses CSS-first configuration. `build.rs` builds the bundle into `OUT_DIR`, so `cargo build` cannot embed stale files. Use `just ui` or `just ui-dev` to work on the UI alone.
- Added media details for container, file size, overall bitrate, resolution, dynamic range, bit depth, and every video, audio, and subtitle track. Dynamic range identifies HDR10, HLG, and Dolby Vision. Tracks include codec, language, and channel layout. The new `/api/item/{id}/media` endpoint reads `MediaSources` from Jellyfin when requested because the local catalog does not store codec data. Subtitle lists collapse after six tracks.
- Added external links to library and discovery details. The "More info" menu links movies and series to exact IMDb, TMDB, TVDB, and Trakt pages when ids exist. Movies also link to Letterboxd. Library links use ids from the local catalog, while direct and Companion discovery use Seerr's IMDb and TVDB ids. Invalid ids and providers without a page for that media type are omitted. Rotten Tomatoes has no id-based title URL, so its entry is clearly labeled as a title-and-year search.
- Added Play to series details. It names and starts the show's Next Up episode. If the show is complete or Jellyfin cannot provide Next Up, it starts the first episode.
- Added `scripts/cdp-screenshot.ps1` to capture an app window started with `--remote-debugging-port`.
- Added the Seerr integration's REST client, error type, and storage. `POST /api/seerr/connect` rejects instances with an unfinished setup wizard because the first login would become the owner. It also rejects instances that do not use Jellyfin. `GET /api/seerr/status` reports link state, request permissions, auto-approval permissions, and quotas by media type. Each link uses the current user's Seerr session and is tied to the matching Jellyfin account. Signing out or changing users removes it. The client captures cookies during the first probe for CSRF-protected instances and retries GET requests only. No UI yet.
- Added `--seerr-status` to check the Seerr link without starting the browser shell.
- Added Seerr password linking and unlinking. `POST /api/seerr/link/password` uses the media-server password once and never stores it. `POST /api/seerr/unlink` ends the Seerr session, removes the local link, and keeps the address for later use. MediaFlick checks the returned Seerr account against the current Jellyfin account. It logs out and discards a session for any other user. Error messages now distinguish a rejected password from a user who has not been imported into Seerr.
- Added `GET /api/seerr/search`, `GET /api/seerr/discover/{trending|movies|tv}`, and `GET /api/seerr/media/{movie|tv}/{tmdbId}`. Results match the local cache by media kind plus TMDB id because TMDB can assign the same number to a film and a series. For example, id 603 identifies two unrelated titles. MediaFlick drops `person` results before matching. The `GET /api/seerr/image/{size}/{file}` proxy caches TMDB posters on disk and accepts only a rendition size and plain TMDB file token.
- Added Discover with Trending, Popular films, and Popular series tabs, Seerr search, and a request dialog. The dialog supports per-season requests when the instance allows them. It shows the 4K option only when both the instance and user permissions allow it, and confirms whether Seerr approved or queued the request. Seerr setup now runs from the user menu. It asks for a password only when passwordless linking is unavailable. Discover and Requests remain hidden until Seerr is linked.
- Added Seerr discovery details with summaries, trailer links, cast, ratings, production data, and regional release dates. Users with Seerr's advanced-request permission can select a Radarr or Sonarr quality profile before requesting a title.
- Added a quick-request button to Seerr discovery posters. It appears on mouse or keyboard focus and stays visible on touch devices. The button opens the existing request dialog without navigating to details, but only when the title has an allowed standard, partial-series, or 4K request.
- Expanded Discover with Seerr's upcoming film and series feeds, illustrated genre browsing for both media types, sorting by popularity, rating, or date, minimum-score filters, trending controls, and a local-library filter. Search loads later result pages while scrolling. Cards show their TMDB score and a clear Request or Open details action. Direct Seerr sessions and Companion use the same allowlisted contract.
- Added a decade filter to Seerr's film and series catalogs. It works with genre, rating, and sort controls. Date ranges include the full decade, except the current decade ends today. The URL keeps the tab and filters while opening a title and returning to Discover.
- Added Requests for the signed-in user's own Seerr requests. It shows approval and download status, allows cancellation while pending, and links to titles after they enter the library. The page refreshes every 30 seconds while open and when the window regains focus, with no background polling. Library search now ends with a separately loaded "Not in your library" block. Local FTS results still render directly from SQLite, and cached titles appear only once. Partially available series now have a "Request season" action on details.
- Fixed Seerr's ambiguous 401 responses. Seerr uses 401 for both an expired session and a valid session without permission. MediaFlick now checks `/api/v1/auth/me` before treating a 401 as expiry. Permission errors leave the link intact. Only a session that fails the identity check prompts the user to link again.
- Added one shared hover preview for posters and progress cards on the home page and in the library. After a short mouse hover, it shows landscape artwork, an optional wordmark, Play, My List, watched, and details controls, plus the rating, maturity rating, year or season count, runtime, and genres. The preview renders outside shelf clipping and closes on scroll, Escape, or navigation. It does not activate for touch or while dragging a shelf.
- The home billboard now rotates through up to five randomly selected movies with landscape artwork. After five seconds, it starts a muted trailer when one is available. It prefers a local Jellyfin trailer through the authenticated byte-range proxy, then falls back to Jellyfin's privacy-enhanced YouTube trailer. The trailer fills a full-height 16:9 panel on the right and blends into the dimmed full-width backdrop. YouTube titles and controls remain cropped out. A title stays until its trailer ends, while one without a usable trailer advances after 12 seconds. Reduced-motion mode shows the still image. Hover or keyboard focus pauses rotation after playback. The navigation dots switch titles and stop the current trailer. Backdrops and trailers load only when their slide becomes active.
- Added genre shelves to the end of the home page. A "Because you watched" shelf uses the leading genre of the first Continue Watching item. It excludes that item and its series, and the chosen genre does not appear again below it.
- Added transparent title wordmarks where Jellyfin provides them. The billboard, hover preview, and details header use the wordmark while keeping the text heading in the document for accessibility. Missing or broken artwork falls back to text.
- Browsing cards now show the server's community rating as a "% match" score. Titles without a score have no badge.
- Added passwordless Seerr linking for supported instances. `POST /api/seerr/link` starts Jellyfin Quick Connect, approves the code through the current Jellyfin session, and lets Seerr redeem it. This requires Seerr newer than v3.3.0 and Quick Connect enabled in Jellyfin. If either is unavailable, the API tells the user to use a password. `POST /api/seerr/link/poll` completes attempts that Seerr has not processed yet.
- Added a startup screen with the MediaFlick logo and a progress bar. The authenticated home page loads its initial data and visible artwork behind the screen before it fades out.
- The startup screen now reports first-time SQLite cache progress, including processed and total Jellyfin item counts. It also explains that later starts are faster.
- Added a Jellyfin WebSocket connection for live library and watch-state updates. Changes from Jellyfin Web, another client, or another device now arrive without waiting for a sync sweep. The connection sends the token in the same authentication header as REST calls, answers keep-alive probes, reconnects with backoff, and requests a reconciliation sync after every connection. Periodic sweeps remain as a backup.
- Made MediaFlick Desktop a Jellyfin cast target. It announces media-control support whenever the event stream connects and appears in Jellyfin's "Play On" menus. Remote Play starts movies and episodes directly, starts a series at its Next Up episode, and starts a season at its first episode. Both mpv and MPC-HC support remote pause, resume, play or pause, stop, seek, next episode, volume, and mute commands. MediaFlick logs and ignores unsupported queue commands instead of applying part of them. It stops advertising the session when the app disconnects.

### Changed

- The home billboard now starts trailers earlier. A slide's player mounts and buffers underneath the establishing still as soon as the slide appears, footage is revealed once playback is confirmed after a 2.5-second establish period instead of five, and the next title's trailer record is prefetched during the current slide so transitions do not wait on an API round trip.
- Sidebar library search and Discover search now update automatically after a 200 ms typing pause, start at two characters, clear immediately below that threshold, and replace URL history instead of waiting for Enter.
- Split oversized Rust modules into smaller player, library, integration, synchronization, preferences, and CEF components without changing their public contracts.
- Reduced the local library cache to a catalog index. Each movie, series, season, or episode keeps one row with identity, titles, year, runtime, community rating, hierarchy, provider ids, genres, and image tags. The details page fetches synopses, cast, critic scores, tags, and studios from `/api/item/{id}/about`. The billboard fetches its synopsis after rendering the cached item, and season requests include episode synopses. MediaFlick does not persist these responses. Their views show loading or error states when Jellyfin is unavailable.
- Limited each live metadata request to its job. The billboard requests a synopsis without user or image data. Detail responses keep the first 24 cast credits and a bounded crew sample instead of every person and headshot returned by Jellyfin.
- Added batched live requests for card resolution, dynamic range, and audio badges. Visible cards register with the scheduler, which combines them into bounded `/api/technical/batch` calls and cancels a batch when none of its cards remain visible. The badges work for movies and series. Request failures do not block browsing, and Appearance can stop these requests by hiding media information.
- Exact Jellyfin person searches now add a live "Featuring" section with that person's movies and series. Cast pages and search use the same identity lookup. The local index no longer stores cast names or overview text, while title and genre search remains in SQLite.
- Changed the full library scan from weekly to daily so in-place metadata edits reach the catalog sooner. The smaller index keeps the scan cheap.
- Bumped the library schema to 13. There is no pre-1.0 migration. MediaFlick deletes older databases and rebuilds the catalog, while preserving the signed-in session and device id needed for the sync.
- Replaced abbreviated rating labels with source icons on cards, hover previews, and title details. Critic and audience scores have separate marks and accessible source names. Hover previews also show a larger video and audio summary.
- Appearance can now hide technical video and audio details on library cards. Its preview shows a sample shelf with the selected rating and media overlays. Editable preference fields use Save, Discard, and Reset.
- Connected Letterboxd profiles and movie reviews now show the display name from the profile's RSS channel title. The normalized username remains the profile identity and link target.
- Replaced Letterboxd movie review cards with a compact profile row. The Letterboxd mark, five-step personal rating, and display name remain visible. Written reviews open on mouse hover or keyboard focus. The app selects ratings and reviews separately, so a new rating-only rewatch does not hide an older review that is still in the feed.
- Renamed the right-click "Client Settings" shortcut to "Settings" and pointed it to the React Player page instead of the native dialog. The settings UI now tracks native file selection and mpv installation requests through matching shell event ids.
- Continue Watching now refreshes a stopped item from Jellyfin after the final playback report. Resume state therefore follows the server's configured thresholds instead of treating every nonzero position as resumable.
- Replaced "% match" labels on browsing cards with ten-point star ratings. These values are community or TMDB metadata, not personal recommendations.
- Removed the README's separate AI-assistance disclosure. The project now treats AI-supported development as part of its normal workflow.
- Discovery tabs now use the full screen and load more titles as the user scrolls. Trending, Movies, and Series remain separate tabs.
- The home billboard now uses half of the available content height, with a minimum for short windows. Its loading skeleton uses the same dimensions and leaves room for an active player bar.
- Changed the billboard's backdrop and trailer overlap to an asymmetrical film-burn mask with a green fringe. A slightly wider video frame and earlier blend give the trailer more room without covering the title.
- Seerr discovery, requests, status, and cancellation now use a shared provider interface. MediaFlick prefers a compatible Companion plugin and falls back to the user's direct Seerr session when the plugin is absent. Desktop still matches results to the local library. Companion writes are not retried after an uncertain response, and Seerr permission failures cannot expire the Jellyfin login.
- Replaced the hand-written ES-module UI with React. The build emits fixed `app.js` and `app.css` files without code splitting because the binary embeds and serves them from memory. `static_asset` now embeds files from `OUT_DIR`, and client-side routing uses `pushState` instead of hash URLs.
- Building now requires Node and pnpm. CI and release jobs install both before running Rust steps. Set `MEDIAFLICK_DESKTOP_SKIP_UI_BUILD=1` to embed a bundle built separately.
- Simplified Settings. It now opens on Player instead of an overview page. The navigation shows page names without repeating their descriptions, card rating sources appear only under Appearance, and the MDBList Ratings page uses plain language for credentials and status.

- The browser window now always loads `mediaflick-desktop://app/`. The shell opens HTTP and HTTPS navigation in the system browser instead of loading Jellyfin's URL in the app.
- Added `rusqlite` with bundled SQLite and FTS5. It remains pinned to 0.37 because the `libsqlite3-sys` build script in 0.40 requires a newer Rust toolchain.
- Raised the Jellyfin request budget to 120 seconds with a 10-second connection timeout. Large sync pages have more time to finish, while unreachable servers still fail quickly during sign-in.
- Rebuilt the details view. It now has a large poster over the item's backdrop, episode breadcrumbs, community and critic ratings, genre links, and a resume bar with time remaining. Lower sections show cast and roles, directors, writers, studios, premiere and added dates, play count, tags, and media details.
- The details view now draws the full backdrop behind the whole page instead of cropping it to the header. Cast, seasons, and details render over the image without moving to make room. A text-sized scrim preserves header contrast, and a second gradient fades the bottom of the image into the page color.
- Merged Next Up into the Continue Watching row. In-progress items come first, followed by each other series' next unwatched episode. If an in-progress episode is also Next Up, it appears once.
- Continue Watching and Next Up now use 16:9 cards with episode stills or landscape title art instead of portrait crops. Episode lists and details use the same format.
- Applied the home page styling to the library, search, discovery, requests, details, sign-in, dialogs, empty states, and player bar. These views now use the same hierarchy, translucent panels, and card motion.
- Changed the app's palette and geometry. Bright green replaces red, and neutral colors carry a green tint. Artwork uses 2px corners, controls use 4px corners, and only progress tracks use pill shapes. Runtimes, ratings, years, counters, and shelf labels use a widely spaced monospace face with accent separators. Shelf headings have an accent marker and trailing rule. Hovered cards use corner brackets instead of scaling, and preview controls are square.
- Watched titles now use a three-pixel progress line along the bottom of the artwork instead of a corner badge. It matches playback progress without covering the image.
- Moved episode browsing onto the series page. A horizontal season row acts as the selector and places Specials last. The selected season shows 16:9 episode cards with episode number, title, community rating, runtime, Play, and watched controls. Synopses remain on episode details. Cards also support hover previews, technical badges, artwork fallbacks, and watched or playback progress. The app selects the season containing Next Up and marks that episode. The URL stores the season selection, and old season routes redirect to it. Episode details now show stills at 16:9.
- Reduced the sign-in header to the MediaFlick logo. Removed the placeholder film icon, eyebrow text, slogan, and introduction.
- Replaced custom Settings toggles and rating-source checkboxes with the shadcn/ui Switch and Checkbox components.
- The app surface no longer behaves like a browser document. Dragging, double-clicking, and select-all never select UI text, while text inputs keep native editing behavior. Dropping files or links onto the window does nothing, page zoom through Ctrl+wheel, pinch, and Ctrl± is pinned, reload, print, save, and view-source shortcuts are ignored, trackpad history swipes no longer navigate, artwork cannot be dragged, and the cursor stays an arrow outside editable fields.
- Revealed scrollbars are now overlay-thin and inherit the signal palette instead of Chromium's light-gray defaults.
- Window titles now follow the open view. Alt-tab and the taskbar show the signed-out gate, Home, Library, Releases, Discover, Requests, Settings, or the open movie or series instead of a static app name.
- The Appearance live preview renders the app's own components — real MediaCards over the cached Continue Watching and Recently Added rows — inside one scoped container that carries the unsaved draft as data attributes and intensity variables. The shared token rules re-skin that subtree exactly as they re-skin the root, so color mode, accent, density, artwork and backdrop intensity, media info, card previews, and the rating-source selection are judged against your own artwork before saving. Resting on a card opens the same expanded preview panel as the live shelves, also themed by the unsaved choices; only that panel's Play, My List, and watched actions stay inert.

### Fixed

- Fixed newly synced titles not appearing at the start of their home shelves. The shelf data already arrives newest-first, but the browser's scroll anchoring kept the rail pinned to the previously visible cards, hiding the new first card off to the left. Rails now opt out of scroll anchoring and reset to their first card on mount and whenever its leading item changes, so shelves start at the first position on launch and a new title takes the first slot with the remaining cards shifting one to the right.
- Fixed the Appearance live preview not responding to the pointer. The preview shelf was fully inert, which removed its cards from hit-testing, so hover brackets, lift, and title color never appeared. Links are now inert individually and the cards use the same home-rail card class as the real shelves.
- Fixed the Appearance live preview not opening the expanded card panel on hover. The preview supplied a stub hover layer, so resting on a card did nothing when card previews were enabled. The shelf now mounts the same preview provider as the live shelves — identical open, close, and hold-open behavior, with the panel portaled outside the page's paint containment and themed by the unsaved draft — while the panel's state-changing actions stay inert so a settings demo cannot start playback or rewrite watch state.
- Fixed YouTube's transient centre pause overlay flashing over billboard trailers. Embedded footage is revealed only after the player itself reports playback, and every playing event restarts the concealment window — including the deferred seek to the embed's start offset. A trailer that never confirms playback or reports an error advances the billboard without its stage ever being shown.
- Fixed Linux taskbars showing Chromium's icon. CEF windows now use the MediaFlick Desktop app id/WM_CLASS and carry embedded title-bar and application-switcher icons instead of relying on desktop-entry matching alone.
- Fixed `just run` and `just run-non-debug` on Linux so the staged CEF runtime directory is added to the dynamic linker path before launching the app.
- Fixed `just run` cleanup on Linux to stop stale MediaFlick CEF subprocesses and warm mpv children despite the executable name being longer than `pkill -x` can match.
- Fixed washed-out hover brackets on the top edge of media cards. The brackets now draw above the ratings gradient and every other card overlay.
- Fixed missing MDBList ratings on lower home-page shelves, especially genre rows. Batch responses had re-registered every mounted card, briefly leaving no cards registered and canceling other batches. The canceled work still held claims on its titles, so replacement requests omitted them. Card registration now keeps a stable identity. An available response can also retry omitted titles a limited number of times.
- Fixed CI and Companion release jobs failing when `global.json` pins a newer .NET SDK than the runner image provides. `setup-dotnet` now installs the pinned version directly from `global.json`.
- Fixed missing backdrops on season and episode details. These pages now request inherited TV artwork from the owning series instead of the child item.
- Fixed card clicks failing when the hover preview appeared between mouse down and mouse up. The preview now treats that release as the card click that started it. Moving onto a card during a drag no longer opens the preview.
- Fixed hover previews missing the selected rating sources because their portal sat outside the ratings context.
- Fixed the full-page card skeleton flashing despite a valid SQLite home cache. The startup cover now waits for the local snapshot. Cached shelves render without waiting for the separate Next Up and billboard requests, which update their own content later.
- Fixed the native main window appearing before its initial route was ready. It now remains hidden until the startup cover has left the rendered page, while load failures still reveal their recovery page and `--hidden` continues to suppress the window.
- Fixed the main window not taking the foreground at startup. The delayed reveal showed the hidden window without activating it, leaving it behind whatever had focus; the reveal now also activates the window.
- Updated id generation to the current `getrandom` fill API.
- Fixed discovery filter changes retaining cards or pages from the previous query. Every server and local-library filter now contributes to one immutable query identity. Changes cancel the old infinite query and remount the result list. Release-decade filters also use full labels and the same 1900 lower bound for films and series in Desktop and Companion.
- Fixed an interrupted initial library sync restarting at zero even though it had saved a page offset. It now resumes from the last committed page, and the progress count no longer moves backward after a temporary failure.
- Fixed the first home page after sign-in showing only Continue Watching without progress. The loading gate now waits for the initial catalog sync before media routes can cache empty home queries.
- Fixed horizontal shelves and raised cards drawing over the sidebar. The content area now clips its paint, keeps a minimum width, and preserves the shelf gutter. The fixed sidebar owns the upper stacking layer.
- Fixed the Seerr table missing after a pre-release build created it under the old Seer name but recorded the new schema version. Startup now repairs that database and removes the unused old table.
- Fixed misleading Seerr errors when an SSO proxy redirects API requests. The client no longer follows redirects. It reports the destination host and asks the user to exempt Seerr's `/api/` paths or use an address that reaches Seerr directly. MediaFlick cannot sign in to an upstream proxy with the user's Jellyfin credentials.
- Fixed Seerr addresses that return HTML reporting a JSON parser offset. MediaFlick now reports the response content type and size without logging or displaying its body. A JSON body with the wrong schema still returns its decode error.
- Tightened Seerr session handling. Logs no longer include upstream response bodies. Poisoned locks keep their existing state, account changes remove the previous user's link, headless checks cannot overwrite newer link changes, and Seerr permission failures do not expire the Jellyfin session.
- Fixed the incremental library sweep writing no rows. `DateLastSaved` is not a valid `ItemSortBy` value, so servers returned empty values and the sweep never advanced its watermark. The sweep now uses `DateCreated`. Jellyfin assigns a new id when a file is replaced, so this also detects replacements.
- Added a weekly full library scan for in-place metadata edits because Jellyfin has no usable changed-since ordering.
- Removed cached items after Jellyfin confirms they no longer exist. A missing item or 404 from `PlaybackInfo` triggers removal. A missing poster first gets a separate existence check because an item can remain valid without artwork. Connection failures never remove cached items.
- Fixed missing poster artwork being requested after every render. The card now switches to its title placeholder after the first failure.
- Fixed deleted episodes remaining on series and season pages as placeholder cards with unusable Play buttons. Opening either page now reconciles its episode list with Jellyfin. New and deleted episodes appear without waiting for the next library sweep. If Jellyfin is unreachable, the cached list remains unchanged.
- Fixed manual library refreshes obeying the hourly deletion gate. Refresh, sign-in, and `--library-sync-once` now bypass the gate, while timer-based syncs still use it.
- Combined the deletion and user-data sweeps because they requested the same pages. Deleted items are now found within an hour instead of once a day without adding requests.
- Fixed severe library grid stutter caused by full-size posters. The UI sent `width`, but the image proxy expects `maxWidth`, so it served images as large as 2000x3000 and 24 MB decoded into 168px slots. A screenful could consume about 1 GB of decoded images. Cards now request the size they display, and old cache entries are fetched once at the smaller size.
- Poster cards no longer render on every scroll event, and images decode off the main thread. The grid recalculates its page window only when that window moves.
- Device ids and bridge session tokens now come from the operating system's CSPRNG. They are no longer hashes of the wall clock and a process counter, which made timed values reproducible.
- Removed `api_key` from direct-play URLs. MediaFlick sends the access token in a header so it stays out of player command lines, recent-file lists, and logs. MPC-HC cannot send headers and still adds the token when it launches.
- The app now accepts `LOCALAPPDATA`, `APPDATA`, `XDG_DATA_HOME`, or `HOME` as its data directory only when the value is absolute. Empty or relative values previously placed `library.db` under the current working directory and could open a different empty database.
- Fixed two early API threads opening separate library databases and starting separate sync threads before the shell was ready. First-time initialization now runs once.
- Jellyfin requests and retries now share one 120-second budget. A stalled server can no longer hold a sign-in or UI request for up to six minutes.
- Added page limits to the library bootstrap and identity sweep. A capped bootstrap resumes from its stored offset on the next cycle. A capped identity sweep skips deletion because an incomplete item list cannot prove that an item is gone.
- Disabled Play, "From start", and episode Play buttons while a launch is pending so a double click cannot start two players.
- Fixed the server address field restoring the saved URL on every keystroke after the user cleared it. The field now stays empty for editing.
- Fixed the details backdrop ending abruptly in windows narrower than about 1560px. The fade position now scales with the 16:9 image height and reaches the bottom at every width.
- Fixed CI and release jobs failing during "Install pnpm". The root has no `package.json`, so `pnpm/action-setup` could not find a version. The UI manifest now pins `packageManager`, and both workflows read it there.
- Fixed Player settings sending the computed `playerConfigured` field, file-picker cancellation clearing executable drafts, and stale picker or installer results updating the wrong request. Native errors now keep their request and target ids for display in Settings.
- Fixed settings updates undoing an unsaved window resize. Also fixed playback pages refreshing before Jellyfin finished updating its cache, and mark-watched hotkey changes waiting for an mpv restart. Preference snapshots now preserve live window bounds, refresh completion invalidates queries, and a running mpv reloads the binding immediately.
- Fixed artwork and backdrop intensity settings having no effect. Billboard autoplay now respects both MediaFlick's reduced-motion setting and the operating system preference. Appearance also has a live draft preview for theme, accent, density, artwork, backdrop, and motion.
- Fixed rating layout in library cards and details. Card source and value pairs now align left without a leading separator or icon indent. Movie and series details show the selected MDBList sources, and episode rows show each episode's Jellyfin community score.
- Fixed several navigation and layout errors. Discover remains active on title details, unknown routes return home, and sidebar search follows queries restored from the URL. Agenda and request loading states keep the final content width. Request cards no longer combine `Unknown` with a specific availability, movie labels are consistent, native dialogs use the saved appearance, and main-frame failures show a MediaFlick recovery page with a safe retry action.
- Fixed rejected Jellyfin tokens going unnoticed when a quiet background request found them. Session expiry now emits one shell event, including when a card-badge or about request detects it, and returns the app to sign-in.
- Added technical quality badges to series and season cards. Jellyfin containers have no streams, so MediaFlick uses the first cached episode in season and episode order, with specials last. Containers without a cached episode are skipped.
- Fixed "Featuring" searches wasting their 12-card limit on season and episode credits. Person queries now request movies and series only.
- Fixed canceled card-badge requests continuing native work after the browser fetch stopped. CEF now sets a cancellation flag, and the handler checks it between Jellyfin chunks.
- Library changes now clear cached technical streams for off-screen cards too. A remuxed item no longer keeps its old badge after remounting.
- The daily full library scan now skips writes for unchanged catalog and watch-state rows. An unchanged library causes no FTS updates or UI query invalidations.
- `/api/item/{id}/about` no longer requests media streams, genres, or user data that it discards. It asks Jellyfin only for the prose, cast, tags, studios, and critic score returned by the endpoint.
- Fixed technical badge requests registering every mounted shelf card, racing a saved hidden-media setting, missing parent invalidation after episode changes, and keeping stale streams after a remux. Only visible and nearby cards register. Series and season results invalidate with their episodes, and active results refresh on a limited interval.
- Reduced Jellyfin response sizes for exact-item reads, child reconciliation, Next Up, and the Upcoming fallback. Each request now asks only for the fields its caller uses.
- Fixed child reconciliation and confirmed deletion updating SQLite without notifying active shelves, details, or badges. Child snapshots now commit as one transaction. Deletion events include the item's former parent, season, and series ids before requesting a replacement sync.
- Watched, favorite, and playback-state updates now refresh only summaries, child lists, and Next Up. They no longer refetch cast, synopsis, trailer, or media details.
- Fixed library refreshes repeatedly replacing the home billboard's featured titles and restarting its background trailer. The selected slides now remain stable for the authenticated session while their user-data controls still update in place.
- Fixed Companion tests failing before discovery under .NET 10. Local, CI, and release jobs now select Microsoft Testing Platform and pass the test project explicitly.
- Fixed all remaining React UI lint warnings. Draft fields now follow their URL or saved-settings source without effect-driven renders, preview and billboard state resets happen during state transitions, metadata schedulers update callback refs after commit, and Fast Refresh modules export components only. The library grid also explicitly opts out of compiler memoization around TanStack Virtual's mutable API.

### Removed

- Removed the background metadata queue and its convergence code, including tables, retry schedules, detail-page priority, and diagnostics in `--library-stats` and `/api/status`. The catalog index and live requests replace them.
- Removed overview, cast, media streams, tags, studios, critic rating, and cast search text from the cached items table. MediaFlick now fetches them when needed.
- Removed the native Client Settings dialog and its injected script.
- Removed settings code used only by the old jellyfin-web bridge, including `SettingsApplyPlan::update_bridge_profile`, `AppSettings::is_complete`, and the playback-context correlation registry.
- Removed the Top 10 shelf. Community scores change too slowly, so it kept showing the same titles.
- Removed the Settings Overview page, the Guided ratings dialog that repeated its source page, and the unused TMDB API key field.
- Removed the old hand-written ES-module UI under `src/shell/ui/app/`. The windowed library grid, filters, Quick Connect, and streaming-quality picker have moved to React, and no build uses the old code. Native dialogs under `src/shell/ui/` remain.
- Removed unused mpv launch helpers, React wrappers, an unused MDBList bearer-auth path, and an unreferenced Companion response type.
- Removed an unused separator component and anti-slop helper. Also stopped exporting TypeScript symbols with no external users.


## [0.1.6] - 2026-07-16

### Added

- Added infinite scroll to the Jellyfin library card/poster grid: scrolling toward the bottom now lazy-loads and appends the next page of items in place and hides the pagination controls, instead of requiring the Next/Previous page buttons. It reuses Jellyfin Web's own paged fetch and card rendering (so cards, images, and auth match exactly) by intercepting the grid container's content updates and appending rather than replacing, so it works across the different library controllers (Movies, TV Shows, and other paged grids). It applies only to the card grid layout (the list/table view keeps its native pager), only takes over once the full pager is present, and degrades to normal pagination if the expected Jellyfin Web DOM is not found.

- Added a `CI` GitHub Actions workflow that runs on every pull request and on pushes to `main`, checking formatting (`cargo fmt --check`) on Linux and running clippy (`-D warnings`), the test suite, and a binary build on both Linux and Windows so dependency and code changes are validated on every PR.
- Enabled Renovate auto-merge for non-major dependency updates and lock-file maintenance: once the `CI` checks pass, Renovate merges these PRs itself (`platformAutomerge` disabled), while major updates still open a PR for manual review.
- Added selectable streaming quality for external playback: keep original-quality direct playback, use Jellyfin's automatic connection limit, or choose a fixed bitrate from 1.5 to 120 Mbps. Auto and limited modes now advertise HLS transcoding support and let Jellyfin fall back to a server transcode when required, while applying saved changes to the next playback without reloading the app.

### Changed

- Rewrote `README.md` to lead with the external-mpv differentiator (SVP4 motion interpolation, SDR-to-HDR, shaders, full `mpv.conf`) rather than a generic "Jellyfin Web in a window" framing, and added a downloads badge.
- Demoted the high-frequency Jellyfin playstate log lines (state send, state sent, and progress-report-due) from `debug` to `trace` so the default `debug` log level is no longer dominated by them.
- Expanded the `sending mpv command` log line so `set_property` commands now report the property and its value inline (for example `set_property pause=true`), summarizing large array/object values like `chapter-list` as an item/field count instead of dumping them.
- Changed the Client Settings "Log level" control from a free-text input with suggestions to a proper dropdown listing Error, Warn, Info, Debug, and Trace.
- Updated the Rust `cef` crate to v149.2.0 ([#16](https://github.com/phob/MediaFlick-Desktop/pull/16) by [@renovate](https://github.com/apps/renovate)).
- Updated the Rust `cef` crate to v149.3.0 ([#17](https://github.com/phob/MediaFlick-Desktop/pull/17) by [@renovate](https://github.com/apps/renovate)).
- Updated the Rust `sevenz-rust2` crate to v0.21.2 ([#18](https://github.com/phob/MediaFlick-Desktop/pull/18) by [@renovate](https://github.com/apps/renovate)).
- Updated the Rust `sevenz-rust2` crate to v0.21.3 ([#19](https://github.com/phob/MediaFlick-Desktop/pull/19) by [@renovate](https://github.com/apps/renovate)).
- Refreshed locked Rust dependencies ([#20](https://github.com/phob/MediaFlick-Desktop/pull/20) by [@renovate](https://github.com/apps/renovate)).
- Refreshed locked Rust dependencies ([#22](https://github.com/phob/MediaFlick-Desktop/pull/22) by [@renovate](https://github.com/apps/renovate)).
- Reorganized the desktop app around playback, player-adapter, Jellyfin, preferences, maintenance, and shell domains. Playback requests, commands, events, context correlation, segment policy, and backend coordination are now backend-neutral contracts shared by mpv and MPC-HC, while CEF and native player protocols remain adapters; added `ARCHITECTURE.md` to document dependency and concurrency rules. The transitional `Mpv*` type aliases and `MpvFullscreenBehavior` were renamed to their backend-neutral `playback`/`preferences` names.
- Client Settings saves now apply only the runtime effects the change actually requires, as computed by `SettingsApplyPlan` (player rebuild and warmup, segment-skip policy update, bridge profile refresh, scrollbar CSS), and the save confirmation states when a log-level change needs an app restart to take effect.

### Fixed

- Replaced mpv's unconditional Windows minimize/restore focus pulse with a native foreground-activation fast path that finds the player window by process ID, restores it only when actually minimized, and verifies activation before retaining the previous pulse as a compatibility fallback. macOS, X11, and Wayland behavior is unchanged.
- Fixed selectable streaming quality sometimes being ignored because Jellyfin's PlaybackInfo query-string bitrate overrode the patched request body, and restored external bitmap-subtitle negotiation (including MPC-HC's encode fallback) so a quality limit does not silently remove subtitle paths that direct playback already supported.
- Fixed rejected mpv commands being mistaken for a dead IPC transport and tearing down an otherwise healthy player session. A command-reply timeout now poisons that IPC worker so later commands cannot pile up behind a stuck read, transport failures still restart mpv, startup-seek rejections retry after the file settles, and a rejected replacement `loadfile` explicitly resets the stale mpv session so it cannot continue untracked.
- Fixed queued Jellyfin progress reports surviving past a later stopped report or being lost during application exit. Pending progress is coalesced per playback session, stopped supersedes stale progress, and both mpv and MPC-HC shutdown now flush the ordered reporter before the process exits.
- Fixed a reused CEF renderer reinstalling the startup streaming-quality snapshot after settings had changed, and made settings persistence an atomic same-directory replacement so a partial write cannot make the next renderer fall back to defaults.
- Fixed transcoded mpv playback briefly starting at the beginning or aborting instead of reaching Jellyfin's resume position when loading a slow external subtitle occupied or closed the validated command pipe. External subtitles now follow Jellyfin MPV Shim's direct remote-URL, synchronous `sub-add` behavior, but are deferred until the resume seek reaches its target and use a subtitle-specific bounded reply wait while retaining the established two-pipe IPC session; on Unix, command-socket reads now use that per-command deadline instead of the event socket's fixed two-second read timeout, and application shutdown now budgets for the bounded subtitle wait before graceful mpv teardown and final Jellyfin report flushing.
- Fixed mpv playback stalls and replacement races by moving Jellyfin playstate HTTP requests onto an ordered background queue, waiting for and validating real mpv IPC command replies instead of treating pipe writes as acceptance, and preserving incoming pending playback when mpv emits the replaced file's stale `end-file` event.
- Fixed opening the app menus (Client Settings, About, Exit) or using the Client Settings buttons (Save, Browse, Get mpv, mpv.io link) during playback tearing down the active mpv session: the Jellyfin video backdrop vanished, the on-screen controls stopped responding (the back button became unclickable), and stopping mpv no longer reported back to Jellyfin. These bridge actions navigated the page via `window.location`, which fires Jellyfin Web's `beforeunload` handler — stopping and destroying the current player and orphaning the running mpv — before CEF cancels the navigation. They now use the same no-cors request the rest of the bridge already uses, which does not unload the page.
- Fixed the CI build-and-test jobs failing to resolve the CEF cache path by defining `CEF_PATH` at the job level.
- Fixed privileged resource-request bridge actions running directly on CEF's IO thread by marshalling them to the UI thread, and required the per-session bridge token for local welcome and data-page actions as well as Jellyfin-origin actions. Playback-context registration stays synchronous on the IO thread so a directly following stream capture always sees it, and rejected or unrecognized bridge requests now log their URL with the session token redacted.
- Fixed late playback context for another item being merged into MPC-HC's active Jellyfin reporter, and made playback IDs monotonic across runtime backend switches.
- Fixed player replacement and shutdown holding shell-state or coordinator locks during bounded process teardown, preventing long settings and CEF callback stalls. Retired backends now tear down on a detached thread so switching player backends never blocks the CEF UI thread, and a poisoned coordinator lock no longer silently drops player commands or leaks the running player.
## [0.1.5] - 2026-06-24

### Added

- Added in-app mpv setup: the welcome and Client Settings screens now offer a one-click "Download mpv" on Windows (fetched and extracted from the shinchiro mpv builds) and copyable per-OS install commands on macOS and Linux, plus a link to mpv.io/installation.
- Added native Jellyfin intro and credits skipping in mpv with prompt/always settings and forward-seek prompt acceptance.
- Added Jellyfin recap and commercial segment skipping in mpv with their own prompt/always Client Settings, alongside the existing intro and credits options.
- Added skip-segment markers on the mpv seek bar by injecting chapter ticks at each segment's boundaries, so the timeline shows where intros, credits, recaps, and commercials are skipped. Existing embedded file chapters are preserved and merged with the markers rather than replaced.
- Added an MPC-HC player backend on Windows: a new "Player backend" client setting switches playback between mpv and MPC-HC, driven over slave mode (`WM_COPYDATA` / `MpcApi.h`) — open, resume, play/pause, seek, playback speed, audio/subtitle track selection, segment skipping with on-screen prompts and auto-skip countdowns, Jellyfin progress reporting, and end-of-file auto-advance to the next episode. The setting is a segmented switch that shows only the active backend's options and applies live without restarting the app; MPC-HC launches on first playback rather than at app start. Both players sit behind a new `PlayerBackend` trait with a capability probe. On MPC-HC, external subtitles play by requesting a server-side burned-in transcode, volume and mute are emulated through relative volume steps, and the configured default fullscreen behavior is applied on every load (matching mpv); chapter-marker pips and the mark-watched hotkey remain mpv-only and degrade gracefully. On every file open MediaFlick asserts the selected audio/subtitle track (translating mpv's 1-based track ids to MPC-HC's 0-based indexes) and the resume position. Because MPC-HC services `CMD_SETPOSITION` with a synchronous seek that can block its window for many seconds while it re-fetches a remote stream (the duration depends heavily on the file — sub-second on some, ~15s on high-bitrate 4K), all slave-mode commands are dispatched on a dedicated sender thread so the app stays responsive during the seek, rapid seeks are coalesced to the latest target, and a "Seeking..." on-screen message is shown until MPC-HC reports the seek completed.
- Added unit tests for the CEF bridge origin allowlist, external-link scheme checks, and U+2028/U+2029, HTML, and percent-encoding escaping helpers, locking in recent security hardening.
- Added a single-instance gate so only one MediaFlick session runs at a time: a stable id is persisted in `instance.json` (mirroring Jellyfin Desktop's instance file) and used to name a Windows mutex acquired at startup. A second launch detects the existing session, shows an "already running" message, and exits without starting a duplicate player or WebUI. CEF subprocesses are unaffected.
- Added a reusable in-app error toast that surfaces user-facing failures as a dismissible notification injected into the WebUI (styled to match the update toast), with a Copy button to put the error text on the clipboard. It stays on screen until dismissed, and reports when playback is requested but no media player is configured, and when a player backend fails to start playback — mpv or MPC-HC failing to launch (for example a wrong executable path) or mpv rejecting the video — instead of failing silently in the log.

### Changed

- Redesigned the first-run welcome screen to match the app's dark, Jellyfin-compatible design system: removed the marketing-style gradient background, unified colors and typography with the settings dialog, reserved the violet→cyan gradient for the brand mark, and integrated the "get mpv" setup inline instead of as a nested card.
- Stopped bundling mpv in the Windows installer and zip; the app now downloads mpv on first run or guides the user to install it, producing a much smaller installer.
- Changed automatic intro and credits skipping to show a three-second countdown before seeking.
- Moved app-owned dialog and load-error markup templates out of Rust source files.
- Slimmed the README to a Why, Features, and short Install section, and moved the build instructions into a dedicated `BUILDING.md`.
- Replaced the About and Client Settings dialog brand marks with the app logo.
- Polished the About dialog and redesigned the update notification as a compact pill without installer filename copy.
- Changed the default CEF cache location to the project-local `.cache/cef` directory instead of an upstream Jellyfin Desktop checkout path.
- Updated the Rust `cef` crate to v149.1.0 ([#12](https://github.com/phob/MediaFlick-Desktop/pull/12) by [@renovate](https://github.com/apps/renovate)).
- Updated the draft release workflow to use `actions/cache@v6` ([#14](https://github.com/phob/MediaFlick-Desktop/pull/14) by [@renovate](https://github.com/apps/renovate)).
- Updated the Rust `sevenz-rust2` crate to v0.21.1 ([#15](https://github.com/phob/MediaFlick-Desktop/pull/15) by [@renovate](https://github.com/apps/renovate)).
- Changed the Windows auto-update installer launch to use Inno Setup `/SILENT` instead of `/VERYSILENT`.
- Changed the Windows in-app mpv download to install mpv inside the app installation directory.
- Changed Windows mpv window raising on file load to pulse the `window-minimized` IPC property so the player window takes focus, instead of the `ontop` pulse which only changed z-order.
- Scoped the Jellyfin page `fetch` and `XMLHttpRequest` hooks to PlaybackInfo, play-state report, and direct-stream URLs, so unrelated page requests pass straight through to the native implementation.
- Reworked the Client Settings dialog into Player, Playback, and Application tabs so it no longer grows into one long scroll when a backend's options expand, and the backend-dependent options stay confined to the Player tab. Added a scroll-edge fade and chevron hint that appears when a section overflows the window, so there is still a visual cue that more settings exist below the fold even when scrollbars are hidden app-wide (the `--hide-scrollbars` renderer flag otherwise suppresses the dialog's own scrollbar). The tab strip is keyboard navigable with arrow keys.

### Fixed

- Fixed the Linux and macOS release builds failing to compile because the `warnings = "deny"` lint flagged the Windows-only MPC-HC segment helpers and mpv auto-download phases as dead code on those platforms.
- Fixed every native bridge message (player commands, playback context, play-state reports) being delivered twice because `sendBridgeRequest` fired both a `fetch` and an `<img>` request as a fallback pair; it now sends one and only falls back to the image when `fetch` is unavailable. This was duplicating each play/pause/seek/volume command — and the duplicate seeks compounded MPC-HC's synchronous seek stalls.
- Fixed the external player (mpv or MPC-HC) sometimes being left running after MediaFlick exits — for example after switching player backends at runtime — by binding each spawned player to a Windows job object the OS terminates when the app process ends.
- Hardened the auto-updater to download installers only over HTTPS from GitHub-owned hosts and into a unique per-run directory, preventing redirect-to-untrusted-host and predictable-temp-path attacks.
- Restricted the native `mediaflick-desktop://` bridge to pages from our own local UI or the configured Jellyfin origin, so unrelated page content can no longer drive mpv, settings, or app exit.
- Restricted Jellyfin playback-state reporting to `http(s)` targets and percent-encoded the media-segments item id, closing SSRF and path-injection vectors from page-supplied stream URLs.
- Restricted the configured server URL and externally opened links to `http`/`https` (links also allow `mailto`), rejecting `file:`, `data:`, `about:`, and other schemes.
- Pinned the Windows command-processor shim to the system `cmd.exe` resolved from `SystemRoot` instead of an attacker-settable `COMSPEC` override.
- Surfaced mpv IPC command rejections in the log instead of silently treating every command as successful.
- Fixed an auto-skipped intro/credits segment being consumed even when its seek failed, so the skip can be retried after the mpv session recovers.
- Escaped U+2028/U+2029 in data injected into the Jellyfin page, preventing server-derived playback fields from breaking the injected script.
- Fixed native mpv forward seeks not accepting an active skip-intro/credits prompt when mpv reports the seek event before the `seeking` property.
- Fixed the in-app mpv download deleting a working install before the new archive is validated; the build is now extracted to a staging directory, checked for `mpv.exe`, and swapped into place with the previous install kept until the swap succeeds.
- Fixed segment chapter markers overwriting a file's embedded chapters when a duration or media-segment update raced ahead of mpv's `chapter-list` event; marker injection now waits until the file's own chapters have been captured.
- Fixed Linux and macOS update notifications by linking the updater dialog to the GitHub latest release page instead of offering unsupported automatic installation.
- Fixed Linux and macOS first launch by auto-detecting a system `mpv` executable and using generic mpv executable wording in app UI.
- Fixed Linux AppImage startup aborts with `close symbol missing` by preloading bundled CEF and stripping that preload from spawned mpv processes.
- Fixed the mpv window staying at full-screen size after leaving fullscreen when playback starts fullscreen, by constraining the windowed size with `--autofit=70%`.
- Added a Jellyfin Web integration check that logs a clear console error and shows a dismissible banner when the bridge cannot install its required hooks (for example, after an incompatible Jellyfin Web update), instead of silently failing to drive mpv.
- Required a per-session token on `mediaflick-desktop://` requests originating from the Jellyfin page, so in-origin scripts (such as a rogue Jellyfin plugin or injected content) can no longer forge bridge actions like app exit or playback control.
- Fixed mpv commands silently failing after a half-open IPC connection by detecting a dead command-writer and restarting the mpv session, instead of waiting for the event stream to also disconnect.
## [0.1.4] - 2026-06-19

### Added

- Added a `just test` recipe for running the Rust test suite.

### Changed

- Moved mpv episode transition handling into a dedicated playback transition module.
- Changed mpv lifecycle management to warm a hidden idle IPC process when an executable is configured and to reuse it until the configured mpv path changes or the app exits.
- Changed external mpv raise handling to rely on mpv's own `--focus-on=all` support on Linux/macOS plus a temporary IPC `ontop` pulse instead of Win32 window activation.
- Updated the Rust `cef` crate to v149 ([#10](https://github.com/phob/MediaFlick-Desktop/pull/10) by [@renovate](https://github.com/apps/renovate)).

### Fixed

- Fixed next-episode handoff snapshots reusing the previous episode's final playback position before the new mpv file finished loading.
- Fixed late Jellyfin play-session context not being merged into active mpv playback reports after an external-player handoff.
- Fixed the external-player backdrop remaining above Jellyfin Web after stopping playback from mpv during an auto-started next episode.
- Fixed stale mpv EOF/mark-watched stop events from ending the newly started next episode by correlating WebUI stop handling with playback IDs and Jellyfin item/session identifiers.
- Fixed mpv session supervision so the configured idle IPC process is polled, restarted after process or IPC loss, reconnected once before media handoff commands fail, and cancelled cleanly during app shutdown.
- Fixed cold and slow mpv startup being treated as unavailable too quickly by extending IPC/media readiness waits, logging when mpv exits before creating its IPC pipe, and keeping watched-next handoffs on the existing mpv IPC session while ignoring stale browser stop commands.
- Fixed automatic next-episode playback after mpv reaches EOF by keeping the warm mpv handoff protected and emitting Jellyfin Web's stopped player event with ended stop details.
- Fixed mpv `q`/window-close handling to stop only the current file, keep the warm mpv process alive, and restart the idle process if it exits unexpectedly while the app is running.
- Fixed app shutdown so the controller waits for the warm external `mpv.exe` process to quit or be killed before the app exits.
- Fixed Jellyfin Web playstate synchronization after mpv-driven `q`, `w`, and EOF stops by preserving final player state until Jellyfin handles the stopped event.
- Fixed the external-player backdrop/white-background layering by keeping the synthetic player backdrop above Jellyfin's page background while placing the video OSD above it.
## [0.1.3] - 2026-06-17

### Added

- Added a Client Settings dialog for mpv path browsing, log level, default fullscreen behavior, close behavior, scrollbar visibility, and the mark-watched-next input binding.
- Added Windows auto-update checks with an in-app update toast, download progress, quiet installer launch, and automatic restart into the updated version.
- Added Linux AppImage and macOS DMG packaging scripts and release artifacts to the draft release workflow.

### Changed

- Reduced About and Client Settings dialog copy to keep app-owned surfaces terse.
- Redesigned the About dialog with MediaFlick brand treatment, clearer product copy, metadata grouping, and improved keyboard focus behavior.
- Redesigned the Client Settings dialog with grouped controls, Jellyfin-compatible dark styling, stronger focus states, and clearer save/error feedback.
- Changed draft release automation to build all required platform artifacts before committing and tagging a release.
- Updated draft release workflow actions to `actions/cache@v5` and `actions/checkout@v6` ([#2](https://github.com/phob/mediaflick-desktop/pull/2), [#3](https://github.com/phob/mediaflick-desktop/pull/3), [#5](https://github.com/phob/mediaflick-desktop/pull/5), [#6](https://github.com/phob/mediaflick-desktop/pull/6) by [@renovate](https://github.com/apps/renovate)).
- Updated draft release workflow runners and artifact actions to Ubuntu 24.04, `actions/upload-artifact@v7`, and `actions/download-artifact@v8` ([#7](https://github.com/phob/mediaflick-desktop/pull/7), [#8](https://github.com/phob/mediaflick-desktop/pull/8) by [@renovate](https://github.com/apps/renovate)).
- Updated README rationale and streamlined usage documentation.

### Fixed

- Fixed Client Settings labels sitting above their controls after terse copy removal.
- Fixed packaged macOS CEF startup by resolving bundle resource and framework paths from the app bundle layout.
- Fixed non-Windows CEF compilation by matching platform keyboard event types and normalizing CEF enum IDs.
- Fixed Linux and macOS release builds by compiling the hidden command processor shim only on Windows and making packaging scripts executable.
## [0.1.2] - 2026-06-14

### Added

- Added an About dialog showing the app version, git version, and creator.
- Added WebUI fullscreen toggling from the context menu and F11.

### Changed

- Restyled the About dialog to use a compact dark panel.
- Replaced Jellyfin-logo-based app artwork with original MediaFlick Desktop gradient icon artwork across the app, installer, Linux, macOS, setup screens, and README.
- Updated Windows installer dialogs to show the MediaFlick Desktop logo artwork.
- Rebranded the app, package metadata, documentation, release workflow, and Windows artifacts to MediaFlick Desktop.
- Opened new-window and off-server Jellyfin Web links in the system default browser instead of CEF.

### Fixed

- Fixed a duplicate separator in the Jellyfin Web context menu.

### Removed

- Removed Print and View Source from the Jellyfin Web context menu.
## [0.1.1] - 2026-06-14

### Added

- Added the initial Jellyfin-MPV desktop app that embeds Jellyfin Web in a CEF window while routing direct-play media to an external mpv player.
- Added first-run setup for the Jellyfin server URL and mpv executable path, including a native browse action for selecting `mpv.exe` and persisted settings in `%APPDATA%\jellyfin-mpv\config.json`.
- Added command-line options for launching with a Jellyfin server URL and mpv path.
- Added the JavaScript/Rust Jellyfin bridge that intercepts Jellyfin Web playback, resolves stream playback info, launches mpv, and reports Jellyfin playstate updates.
- Added an external mpv IPC controller with command/event pipes, playback progress observation, pause/seek/stop support, and fullscreen mpv launch behavior.
- Added native Jellyfin Web player-control integration so play/pause, seeking, and playback state can control and reflect external mpv playback.
- Added bidirectional playback-state synchronization from mpv back into Jellyfin Web, including progress, pause state, stop/end handling, and saved resume position updates.
- Added bridge logging and synthetic media-readiness events so intercepted browser playback remains visible and debuggable.
- Added rotating app log files with configurable `--log-level`/`--log-file` options and redacted playback diagnostics.
- Added configurable mpv input bindings in `%APPDATA%\jellyfin-mpv\input.json`, including the default `w` binding to mark the current item watched and start the next queued item.
- Added an **Exit application** action to the Jellyfin Web user menu for cleanly closing the desktop app and external mpv controller.
- Added persistent Jellyfin Web window sizing between launches.
- Added platform application resources, including Windows icons/resources, a macOS app icon and `Info.plist` template, and Linux desktop/metainfo/icon files.
- Added build recipes for debug, release, non-debug run, Windows distribution staging, and Windows installer creation.
- Added Windows release packaging that stages the app with CEF runtime files, locales, and an optional bundled mpv tree.
- Added an Inno Setup installer definition and packaging script for creating `JellyfinMPV-Setup-<version>.exe`.
- Added changelog-driven draft release automation that promotes `CHANGELOG.md` `[Unreleased]` entries into the requested version and creates a draft GitHub release from those notes.
- Added automatic Windows installer and zip artifact builds to the draft release workflow.
- Added Renovate configuration for automated dependency update proposals.
- Added project changelog rules and a `/cl` prompt for auditing unreleased entries before release.
- Added playback regression guard documentation covering the known-good startup/resume behavior.
- Added user-facing README documentation for installation, first launch, usage, mpv configuration, command-line options, local builds, and release packaging.

### Changed

- Reduced playback log noise by keeping frequent mpv position updates (`time-pos`/`playback-time`) out of default debug logs.
- Reorganized the Rust and bridge sources into `app`, `cef`, `jellyfin`, `mpv`, `ui`, and `windows` modules.
- Updated the app to use bundled/default app icons and set the Windows window icon.
- Expanded the README from developer build notes into end-user installation and packaging documentation.

### Fixed

- Fixed resume/startup behavior by waiting for mpv `file-loaded` before seeking to Jellyfin resume positions.
- Fixed mpv handoff URLs by stripping browser-only fragments before sending `loadfile` commands while preserving Jellyfin resume seeks.
- Fixed transient mpv `playback-abort` snapshots so pending loads are not failed before mpv reports `end-file`.
- Fixed Jellyfin Web playback state getting out of sync when mpv stops or reaches the end of an item.
- Fixed packaged CEF startup by wiring subprocess, resource, locale, and Windows GPU runtime settings.
- Fixed watched-state and next-item flow by adding explicit mpv stop handling and a watched-next binding path.
- Fixed watched-next handling to close the current mpv process and let Jellyfin Web's normal autoplay flow decide whether to start the next episode.
- Fixed unwanted Windows console windows from helper script launches by hiding spawned script consoles.
- Fixed app shutdown so the external mpv controller is closed when exiting from the Jellyfin Web UI.
