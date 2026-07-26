import path from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// The bundle is embedded in the Rust binary and served from memory over the
// `mediaflick-desktop://app/` scheme, so there is no network between the bundle
// and the renderer: code splitting and content hashing buy nothing. Emitting
// fixed `app.js` / `app.css` names keeps `static_asset` in
// `src/shell/cef/api.rs` a small match instead of a generated lookup table.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // CEF 150 ships a modern Chromium we control, so no polyfills, no
    // browserslist, no legacy output.
    target: "esnext",
    cssCodeSplit: false,
    // Sourcemaps would be embedded in the binary too; use
    // `--remote-debugging-port` for debugging instead.
    sourcemap: false,
    rollupOptions: {
      output: {
        codeSplitting: false,
        entryFileNames: "app.js",
        assetFileNames: (info) =>
          info.names?.some((name) => name.endsWith(".css")) ? "app.css" : "[name][extname]",
      },
    },
  },
})
