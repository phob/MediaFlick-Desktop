# MediaFlick website

This directory contains the static landing page for `flick.media`. It has no
build step and does not share dependencies with the desktop UI.

## Run locally

From the repository root:

```powershell
python -m http.server 4173 --directory website/public
```

Then open `http://localhost:4173`.

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

The page links to GitHub Releases, so publishing a new desktop version does not
require a website deployment.
