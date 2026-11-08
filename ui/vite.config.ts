import { cpSync, mkdirSync, readdirSync } from "node:fs"
import { createRequire } from "node:module"
import path from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig, type Plugin } from "vite"

const require = createRequire(import.meta.url)
const foliate = path.dirname(require.resolve("foliate-js/view.js"))
const libarchive = path.dirname(require.resolve("libarchive.js/dist/libarchive.js"))
const libarchiveWasm = path.resolve(import.meta.dirname, "../distribution/libarchive-wasm/bundle")

/**
 * What the reader loads at run time, copied verbatim to `dist/reader/`.
 * foliate runs as unbundled ES modules that find their vendor files by
 * `import.meta.url`, and pdf.js and libarchive.js each need a worker script,
 * so none of this can go into `app.js`. libarchive.js's worker and wasm come
 * from `distribution/libarchive-wasm/`, a rebuild without OpenSSL; only its
 * main-thread API comes from the package. `build.rs` embeds exactly what lands
 * here; source maps and license files stay out (the notices are in
 * THIRD-PARTY-NOTICES.md).
 */
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

// The bundle is embedded in the Rust binary and served from memory over the
// `mediaflick-desktop://app/` scheme, so there is no network between the bundle
// and the renderer: code splitting and content hashing buy nothing. Emitting
// fixed `app.js` / `app.css` names keeps `static_asset` in
// `src/shell/cef/api/assets.rs` a small match; the reader's own files under
// `/reader/` come from the table `build.rs` generates.
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
