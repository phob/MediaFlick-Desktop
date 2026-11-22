# 0014. Embedded book reader uses unbundled foliate-js, no-crypto libarchive.wasm, and query-param ranges

Status: Accepted (2026-11-22). Consolidated from codebase audit of `src/books/`, `src/shell/cef/api/books.rs`, `src/shell/cef/api/assets.rs`, `distribution/libarchive-wasm/`, and `ui/src/reader/`.

## Context

MediaFlick embeds a reader for Jellyfin book libraries (EPUB, MOBI, AZW, AZW3, PDF, CBZ, CBR, CB7, CBT) that syncs reading progress with `jellyfin-web`, serves chunked book bytes over CEF's custom scheme without exposing Jellyfin tokens, and decodes RAR/7z/tar comics in-browser without shipping crypto libraries.

## Decision

- **Formats, byte serving, and progress interoperability.** `BookFormat` mirrors the exact nine formats Jellyfin's `BookResolver` accepts; PDF sniffing scans the first 1024 bytes for `%PDF-`. Book bytes are fetched from Jellyfin `/Items/{id}/File` (not `/Download`, which requires download permission), and progress writes go directly to `/UserItems/{id}/UserData` (matching `jellyfin-web`, because Jellyfin's playback session endpoints mark runtime-less books played and zero their position). `PlaybackPositionTicks` matches `jellyfin-web`: `round(read_fraction * 10^7)` for text formats (including fixed-layout EPUB) and `zero_based_page * 10^4` for PDFs and comics; the detail page infers units from filename without downloading bytes. `OpenBooks` gates byte-serving and position-saving routes via a session-generation LRU tracking `synced_ticks` (last confirmed Jellyfin value); `resume()` keeps the local exact location only while Jellyfin still holds `synced_ticks` and lets any changed server value win—explicitly rejecting `max()` so re-reading backwards works. Because CEF intercepts HTTP `Range` headers on custom schemes by skipping into the handler's response, `book_bytes` takes `start`/`end` query parameters instead of a `Range` header.
- **Unbundled `/reader/` engines and CSP.** `foliate-js` resolves vendor files via `import.meta.url` and `pdf.js`/`libarchive.js` need standalone worker scripts, so Vite copies them unbundled into `dist/reader/` (embedded by `build.rs`, omitting license files tracked in `THIRD-PARTY-NOTICES.md`), typed by hand-written `ui/src/reader/engine.d.ts` matching `foliate-js` commit `78914ae`. Dedicated Web Workers inherit their own script's CSP, so `'wasm-unsafe-eval'` is served exclusively on `/reader/libarchive/worker-bundle.js` while the main CEF page keeps refusing WebAssembly.
- **No-crypto `libarchive.wasm` build and comic limits.** `distribution/libarchive-wasm/build.sh` disables all external crypto/hash libraries (RAR5 uses libarchive's built-in CC0 BLAKE2sp), verifies `HAVE_ZLIB_H`/`HAVE_BZLIB_H`/`HAVE_LZMA_H` in generated `config.h` (CMake otherwise silently drops missing codecs), and links with `-sWASM_BIGINT=0 -sEXPORTED_RUNTIME_METHODS=cwrap,HEAPU8` for `libarchive.js 2.0.2` compatibility under Emscripten 6. Vite's `buildStart` enforces an exact version match between npm `libarchive.js` and the vendored worker bundle. Because `libarchive.js` copies archives into WASM linear memory and solid RAR/7z archives decompress front-to-back, CBR/CB7/CBT files over 1024 MiB are rejected up front and smaller archives extract whole inside the worker.
- **`foliate-js` and iframe constraints.**
  - `layoutAttributes` emits computed pixels (not `em`) because `foliate-js` parses attributes with `parseFloat`; typography scales from a `1920x1048` reference viewport (1080p minus 32 px footer, 24 px = 100%, 16 px floor).
  - Embedded book documents cannot read `:root` CSS variables, so the reader resolves and copies `--reader-*` (and `--background`/`--foreground` for the App theme) into each document.
  - `comicLoader` strips `__MACOSX`/hidden files, applies natural numeric sort with raw-name tie-breaking, and renames pages to `00000001.<ext>` because `foliate-js` filters lowercase extensions and sorts lexicographically.
  - `RemoteFile.open` constructs a filename ending with the backend-sniffed format because `foliate-js` distinguishes CBZ from EPUB by extension.
  - Closing a `foliate-js` `View` does not free blob URLs or the `pdf.js` worker, so `book.destroy()` runs on close and error paths.
  - Fixed-layout books compute equal-weight fraction `section.current / (section.total - 1)` because `foliate-js` byte-weights fractions across image sizes.
  - Section iframes have separate realms (`doc.defaultView?.Element ?? Element`), margin clicks target the shadow DOM (`view`) while content clicks target `detail.doc`, `contextmenu` checks `event.button === 2` so keyboard ContextMenu keeps its binding, and `MediaPlayPause`/`MediaPlay`/`MediaPause`/`MediaStop` pressed inside section iframes are re-dispatched onto `window` (while reader claims `MediaTrackNext`/`MediaTrackPrevious` via `preventDefault` and omits focused-widget keys).

## Rejected alternatives

- Passing HTTP `Range` headers to `mediaflick-desktop://app/api/books/...`: CEF re-slices the chunk response itself, breaking reads past chunk 0.
- Enabling `'wasm-unsafe-eval'` in the main document CSP: widens attack surface for the whole UI when only the libarchive worker needs WebAssembly.
- Using `max(local, remote)` for reading progress merge: breaks re-reading a book from an earlier page on another device.

## Consequences

- Upgrading `libarchive.js` requires rebuilding and verifying `distribution/libarchive-wasm/` via `just libarchive-wasm update`.
- Upgrading `foliate-js` requires re-checking `ui/src/reader/engine.d.ts` and the `foliate-js` workarounds above.
