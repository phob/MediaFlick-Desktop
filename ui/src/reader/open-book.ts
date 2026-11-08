import type { FoliateBook, FoliateLocation, View } from "foliate-js/view.js"
import { archiveComic, zipComic } from "./comics"
import { foliateComics, foliateView } from "./engines"
import { RemoteFile } from "./remote-file"

export type BookFormat = "epub" | "mobi" | "azw" | "azw3" | "pdf" | "cbz" | "cbr" | "cb7" | "cbt"

export interface OpenOptions {
  signal: AbortSignal
  onDocument(doc: Document): void
  onRelocate(location: FoliateLocation): void
  onDownload(fraction: number): void
}

async function loadBook(file: RemoteFile, format: BookFormat, onDownload: (fraction: number) => void): Promise<FoliateBook> {
  switch (format) {
    case "cbz":
    case "cbr":
    case "cb7":
    case "cbt": {
      const { makeComicBook } = await foliateComics()
      const loader = format === "cbz" ? await zipComic(file) : await archiveComic(file, onDownload)
      return makeComicBook(loader, file)
    }
    default: {
      const { makeBook } = await foliateView()
      return makeBook(file)
    }
  }
}

/** Opens a book from the server into `host` and shows its first page. */
export async function openBook(host: HTMLElement, itemId: string, format: BookFormat, options: OpenOptions): Promise<View> {
  const { View } = await foliateView()
  // foliate tells CBZ from EPUB by the name, so the name carries the format the bytes were sniffed as.
  const file = await RemoteFile.open(itemId, `${itemId}.${format}`, options.signal)
  const book = await loadBook(file, format, options.onDownload)
  options.signal.throwIfAborted()
  const view = new View()
  view.style.cssText = "display: block; width: 100%; height: 100%"
  view.addEventListener("load", ({ detail }) => options.onDocument(detail.doc))
  view.addEventListener("relocate", ({ detail }) => options.onRelocate(detail))
  view.addEventListener("external-link", (event) => event.preventDefault())
  host.replaceChildren(view)
  await view.open(book)
  await view.init({ showTextStart: false })
  return view
}
