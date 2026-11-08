// The reader's engines run unbundled from the `/reader/` tree, which
// `ui/vite.config.ts` copies verbatim from these packages and `build.rs`
// embeds, so each URL serves exactly the module its type is taken from.
// That is what makes the casts below safe: `import()` of a computed URL is
// untyped, and these URLs are the packages' own files.
const ROOT = "/reader"

export async function foliateView() {
  return (await import(/* @vite-ignore */ `${ROOT}/foliate/view.js`)) as typeof import("foliate-js/view.js")
}

export async function foliateComics() {
  return (await import(/* @vite-ignore */ `${ROOT}/foliate/comic-book.js`)) as typeof import("foliate-js/comic-book.js")
}

export async function foliateZip() {
  return (await import(/* @vite-ignore */ `${ROOT}/foliate/vendor/zip.js`)) as typeof import("foliate-js/vendor/zip.js")
}

export async function libarchive() {
  const module = (await import(/* @vite-ignore */ `${ROOT}/libarchive/libarchive.js`)) as typeof import("libarchive.js")
  module.Archive.init({ workerUrl: `${ROOT}/libarchive/worker-bundle.js` })
  return module.Archive
}
