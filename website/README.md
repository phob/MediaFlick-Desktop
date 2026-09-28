# MediaFlick website

This directory contains the static landing page for `flick.media`. It has no
build step and does not share dependencies with the desktop UI:
`public/index.html`, `public/styles.css`, `public/site.js`, and the screenshots
and loops in `public/media`.

The page follows the app's "Signal" look, with tokens mirrored from
`ui/src/app.css`. `site.js` is optional: without it the page is complete, but
static. It adds scroll reveals, in-view video playback, the hero's readouts,
the Browse walkthrough, tabs, the gallery lightbox, the accent swatches, and
download links for the newest GitHub release. `public/_headers` sets a strict
CSP. Scripts, styles, and media are same-origin only, and the only outside
connection allowed is `https://api.github.com`. Don't add inline scripts,
inline `style` attributes, or third-party assets.

## Motion

Every animation stands for something the app does, and each kind of element
enters the way it is built:

- **Hero.** The headline rises word by word. The readouts around the window
  follow the recording: library ones while it tours Home and a details page,
  playback ones once the film starts (`data-play-at` on the hero video is that
  second; `just website-encode` prints it). The readouts drift at different
  rates with the scroll, and the copy recedes as the window takes over.
- **Reveals.** `data-reveal` on a `.reveal` element picks the entrance: `draw`
  runs a rail out from its mark, `clip` wipes a heading up out of its
  baseline, `lock` settles a screen and flashes the app's corner brackets
  once, `wire` pops the Companion's services in along their wires.
- **Browse walkthrough.** The app window pins while four steps scroll past.
  The step in the middle of the viewport is the frame the window shows; its
  title bar names it, and the hover recording plays only on the hover step.
- **Tracking.** The nav's underline slides to the current section. From 1400px
  wide, a spine on the left lists the sections and fills with the accent as
  the page is read.
- **Gallery.** Shots pan slightly as they cross the strip. Where the View
  Transitions API exists, a thumbnail grows into the lightbox and shrinks back
  on close.
- **Small things.** The ticker speeds up with the scroll and settles again; a
  light follows the pointer across the Details tiles.

Scroll-linked motion uses CSS scroll-driven animations behind `@supports`, so
browsers without them keep the same content with time-based motion only.
`prefers-reduced-motion` disables all of it, including the view transition,
and the walkthrough switches frames without a fade.

## Run locally

From the repository root:

```powershell
python -m http.server 4173 --directory website/public
```

Then open `http://localhost:4173`. This server doesn't apply `_headers`; use
`just website-check` to test with the real CSP.

## Check

```powershell
just website-check
```

`scripts/website/check-site.mjs` serves `public/` with the production CSP. It
drives headless Chrome or Edge at a desktop and a phone viewport, and fails on:

- console errors and CSP violations
- missing or undecodable images and videos
- horizontal overflow
- a hero window below the fold, or headline words and readouts that don't
  follow the recording
- a walkthrough step that doesn't take over the pinned window, name itself in
  its title bar, or play the hover recording only on the hover step
- a nav underline or spine that doesn't track the current section
- broken tabs, accent swatches, or lightbox (open and close)

It writes `build/website-check/report.json`, full-page `desktop.png` and
`mobile.png`, and one viewport screenshot per walkthrough step
(`walk-<viewport>-<step>.png`). Headless Chrome may not start the hero
recording at all; the report notes that under `hero.playback`, and the
library-phase assertion is skipped in that case.

## Regenerate the screenshots and loops

All media comes from the real app, driven over the Chrome DevTools Protocol.
You need `just build`, ffmpeg on `PATH`, and Windows (window sizing uses
Win32).

```powershell
just build
just website-capture demo    # Jellyfin's public demo server, empty throwaway profile
just website-capture home    # copy of your signed-in profile, for Companion features
just website-encode          # raw captures -> website/public/media
just website-check
```

Pass scene names to re-capture only those, for example
`just website-capture demo player tour`. Raw captures and
`report-<profile>.json` go to `build/website-capture/raw`;
`build/website-capture/encode-report.json` lists every encoded file with its
size and dimensions, plus `heroPlayAt`, the second at which the hero recording
cuts to playback. After re-encoding, copy that value to `data-play-at` on the
hero video in `public/index.html`.

How the capture works:

- **Profiles.** Both profiles run under `build/website-capture/profiles` and
  never write to your real one. The `home` copy leaves out `instance.json`, so
  it runs beside a normal session. Before any capture, it replaces the account
  name and server host with neutral stand-ins, forces the default accent, and
  hides Letterboxd reviews, since those come from real people.
- **Window sizing.** CEF tiles screenshots taken under a device-metrics
  override. The script sizes the real window to a 1600×1000 CSS viewport
  instead, and captures at the display's own scale.
- **Built-in player.** libmpv draws the video natively under a transparent
  page, so the player scene captures the page with alpha at the film's exact
  position. `encode.mjs` then composites it over the same seconds of the
  source file. No desktop capture is involved. Audio is muted the moment the
  player appears.

The demo content is public-domain films plus *Caminandes* (Blender Foundation,
CC BY), which the page credits in its footer.

## Deploy with Cloudflare Workers Builds

The repository also contains the desktop app's Vite project under `ui`. Set the
Worker's root directory to `website` so Cloudflare finds this directory's
`wrangler.jsonc` instead of auto-detecting the desktop UI.

Use these settings under **Settings > Build**:

- Root directory: `website`
- Build command: leave empty
- Deploy command: `npx wrangler deploy`
- Non-production deploy command: `npx wrangler versions upload`
- Build watch paths, include: `website/**`

The Wrangler project name is `flick-media`. Change the `name` in
`wrangler.jsonc` if the existing Worker has a different name.

## Deploy with Cloudflare Pages

If the project is a Pages project rather than a Worker, use:

- Framework preset: None
- Root directory: `website`
- Build command: `exit 0`
- Build output directory: `public`
- Build watch paths, include: `website/**`

After the first deployment, add `flick.media` as a custom domain. Add
`www.flick.media` too if you want Cloudflare to redirect it to the apex domain
with a Redirect Rule.

The page links to GitHub Releases and reads the newest version from the GitHub
API, so publishing a new desktop version does not require a website deployment.
