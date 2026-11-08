// Bundles libarchive.js's browser worker around the freshly linked emscripten
// glue, with the rolldown and Comlink that ui/pnpm-lock.yaml pins.
// Usage: node bundle-worker.mjs <ui dir> <worker entry> <output file>
import { createRequire } from "node:module"
import { realpathSync } from "node:fs"
import path from "node:path"

const [ui, entry, output] = process.argv.slice(2)
const fromVite = createRequire(realpathSync(path.join(ui, "node_modules/vite/package.json")))
const fromLibarchive = createRequire(realpathSync(path.join(ui, "node_modules/libarchive.js/package.json")))
const { rolldown } = await import(fromVite.resolve("rolldown"))

const bundle = await rolldown({
  input: entry,
  resolve: { alias: { "comlink/dist/esm/comlink.mjs": fromLibarchive.resolve("comlink/dist/esm/comlink.mjs") } },
})
await bundle.write({ file: output, format: "es", minify: true, comments: { legal: true } })
await bundle.close()
