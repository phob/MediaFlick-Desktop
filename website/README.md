# MediaFlick website

This directory contains the static landing page for `flick.media`. It has no
build step and does not share dependencies with the desktop UI.

## Run locally

From the repository root:

```powershell
python -m http.server 4173 --directory website
```

Then open `http://localhost:4173`.

## Deploy with Cloudflare Pages

Create a Pages project connected to this GitHub repository and use these build
settings:

- Production branch: `main`
- Build command: leave empty
- Build output directory: `website`
- Root directory: leave at the repository root
- Build watch paths, include: `website/**`

After the first deployment, add `flick.media` under the Pages project's custom
domains. Add `www.flick.media` too if you want Cloudflare to redirect it to the
apex domain with a Redirect Rule.

The page links to GitHub Releases, so publishing a new desktop version does not
require a website deployment.
