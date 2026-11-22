import { cpSync, mkdirSync, readFileSync, readdirSync } from "node:fs"
import { createRequire } from "node:module"
import path from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig, type Plugin } from "vite"

const require = createRequire(import.meta.url)
const foliate = path.dirname(require.resolve("foliate-js/view.js"))
const libarchive = path.dirname(require.resolve("libarchive.js/dist/libarchive.js"))
const libarchiveWasm = path.resolve(import.meta.dirname, "../distribution/libarchive-wasm/bundle")
const libarchiveWasmRecipe = path.resolve(import.meta.dirname, "../distribution/libarchive-wasm/build.sh")

const READER_FILES: { from: string; to: string; files: readonly string[] }[] = [
  {
    from: foliate,
    to: "foliate",
    files: [
      "view.js", "epubcfi.js", "progress.js", "overlayer.js", "text-walker.js",
      "epub.js", "mobi.js", "comic-book.js", "pdf.js", "paginator.js", "fixed-layout.js",
      "vendor/zip.js", "vendor/fflate.js",
      "vendor/pdfjs/pdf.mjs", "vendor/pdfjs/pdf.worker.mjs",
      "vendor/pdfjs/text_layer_builder.css", "vendor/pdfjs/annotation_layer_builder.css",
    ],
  },
  { from: libarchive, to: "libarchive", files: ["libarchive.js"] },
  { from: libarchiveWasm, to: "libarchive", files: ["worker-bundle.js", "libarchive.wasm"] },
]
const READER_DIRECTORIES: { from: string; to: string; extensions: readonly string[] }[] = [
  { from: path.join(foliate, "vendor/pdfjs/cmaps"), to: "foliate/vendor/pdfjs/cmaps", extensions: [".bcmap"] },
  { from: path.join(foliate, "vendor/pdfjs/standard_fonts"), to: "foliate/vendor/pdfjs/standard_fonts", extensions: [".pfb", ".ttf"] },
]

function readerAssets(): Plugin {
  return {
    name: "mediaflick-reader-assets",
    apply: "build",
    buildStart() {
      const manifest: unknown = require("libarchive.js/package.json")
      const npm = typeof manifest === "object" && manifest !== null && "version" in manifest ? String(manifest.version) : null
      const vendored = /^\s*"libarchivejs\|([^|"]+)\|/m.exec(readFileSync(libarchiveWasmRecipe, "utf8"))?.[1] ?? null
      if (npm === null || npm !== vendored) {
        this.error(
          `the reader's libarchive.js is ${npm ?? "unversioned"} from ui/package.json, but distribution/libarchive-wasm/build.sh built its worker from libarchive.js ${vendored ?? "(no libarchivejs pin found)"}; ` +
            "pin the same version in both and run `just libarchive-wasm update`",
        )
      }
    },
    writeBundle({ dir }) {
      const out = path.join(dir ?? "dist", "reader")
      const copy = (source: string, target: string) => {
        mkdirSync(path.dirname(target), { recursive: true })
        cpSync(source, target)
      }
      for (const { from, to, files } of READER_FILES)
        for (const file of files) copy(path.join(from, file), path.join(out, to, file))
      for (const { from, to, extensions } of READER_DIRECTORIES)
        for (const file of readdirSync(from).filter((name) => extensions.includes(path.extname(name))))
          copy(path.join(from, file), path.join(out, to, file))
    },
  }
}

export default defineConfig({
  plugins: [react(), tailwindcss(), readerAssets()],
  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "esnext",
    cssCodeSplit: false,
    sourcemap: false,
    rollupOptions: {
      external: [/^\/reader\//],
      output: {
        codeSplitting: false,
        entryFileNames: "app.js",
        assetFileNames: (info) =>
          info.names?.some((name) => name.endsWith(".css")) ? "app.css" : "[name][extname]",
      },
    },
  },
})
