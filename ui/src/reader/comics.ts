import type { ComicLoader } from "foliate-js/comic-book.js"
import { foliateZip, libarchive } from "./engines"
import type { RemoteFile } from "./remote-file"

const IMAGE_TYPES: Readonly<Record<string, string>> = {
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  png: "image/png",
  gif: "image/gif",
  bmp: "image/bmp",
  webp: "image/webp",
  svg: "image/svg+xml",
  jxl: "image/jxl",
  avif: "image/avif",
}

const ARCHIVE_LIMIT_BYTES = 1024 * 1024 * 1024

interface Page {
  original: string
  extension: string
  size: number
  load(): Promise<Blob>
}

function comicLoader(pages: Page[]): ComicLoader {
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" })
  const ordered = pages
    .filter(({ original }) => {
      const parts = original.split("/").filter((part) => part !== "" && part !== ".")
      return !parts.includes("__MACOSX") && !parts.some((part) => part.startsWith("."))
    })
    .sort((a, b) => collator.compare(a.original, b.original) || (a.original < b.original ? -1 : a.original > b.original ? 1 : 0))
  const byName = new Map(ordered.map((page, index) => [`${String(index + 1).padStart(8, "0")}.${page.extension}`, page]))
  return {
    entries: [...byName.keys()].map((filename) => ({ filename })),
    async loadBlob(name) {
      const page = byName.get(name)
      if (!page) throw new Error(`no comic page named ${name}`)
      return new Blob([await page.load()], { type: IMAGE_TYPES[page.extension] })
    },
    getSize: (name) => byName.get(name)?.size ?? 0,
  }
}

function imageExtension(name: string): string | null {
  const extension = name.slice(name.lastIndexOf(".") + 1).toLowerCase()
  return extension in IMAGE_TYPES ? extension : null
}

export async function zipComic(file: RemoteFile): Promise<ComicLoader> {
  const { configure, ZipReader, BlobReader, BlobWriter } = await foliateZip()
  configure({ useWebWorkers: false })
  const entries = await new ZipReader(new BlobReader(file)).getEntries()
  const pages: Page[] = []
  for (const entry of entries) {
    const extension = imageExtension(entry.filename)
    const getData = entry.getData
    if (entry.directory || !extension || !getData) continue
    pages.push({
      original: entry.filename,
      extension,
      size: entry.uncompressedSize,
      load: () => getData.call(entry, new BlobWriter()),
    })
  }
  return comicLoader(pages)
}

export async function archiveComic(file: RemoteFile, onProgress: (fraction: number) => void): Promise<ComicLoader> {
  if (file.size > ARCHIVE_LIMIT_BYTES) {
    throw new Error(`This comic is ${Math.round(file.size / 1024 / 1024)} MB; the reader opens RAR, 7z and tar comics up to 1024 MB.`)
  }
  const Archive = await libarchive()
  const archive = await Archive.open(await file.download(onProgress))
  let tree: unknown
  try {
    tree = await archive.extractFiles()
  } finally {
    await archive.close()
  }
  const pages: Page[] = []
  const walk = (directory: unknown, prefix: string) => {
    if (typeof directory !== "object" || directory === null) return
    for (const [name, value] of Object.entries(directory)) {
      const path = `${prefix}${name}`
      if (!(value instanceof File)) walk(value, `${path}/`)
      else {
        const extension = imageExtension(name)
        if (extension) pages.push({ original: path, extension, size: value.size, load: async () => value })
      }
    }
  }
  walk(tree, "")
  return comicLoader(pages)
}
