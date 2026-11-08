// foliate-js ships no types. These declare the subset the reader uses, as
// read from commit 78914ae; `engines.ts` loads the modules themselves from
// the embedded `/reader/` tree.

declare module "foliate-js/view.js" {
  /** What foliate reads from its input: `view.js`, `pdf.js`, `mobi.js` and zip.js's `BlobReader`. */
  export interface BookFile {
    readonly name: string
    readonly type: string
    readonly size: number
    slice(start?: number, end?: number): { arrayBuffer(): Promise<ArrayBuffer> }
    arrayBuffer(): Promise<ArrayBuffer>
  }

  export interface FoliateTocItem {
    label: string
    href: string
    subitems?: FoliateTocItem[] | null
  }

  export interface FoliateSection {
    id: string | number
    linear?: string
    size?: number
  }

  export interface FoliateBook {
    metadata?: { title?: unknown; language?: unknown }
    toc?: FoliateTocItem[]
    sections: FoliateSection[]
    rendition?: { layout?: string }
    dir?: string
    destroy?(): void
  }

  export interface FoliateLocation {
    fraction: number
    section: { current: number; total: number }
    location: { current: number; next: number; total: number }
    tocItem?: { label?: string; href?: string; id?: number } | null
    cfi: string
  }

  export interface FoliateRenderer extends HTMLElement {
    setStyles?(styles: string | [string, string]): void
    prevSection(): Promise<void>
    nextSection(): Promise<void>
    readonly atStart: boolean
    readonly atEnd: boolean
    readonly page: number
    readonly pages: number
    getContents(): { doc: Document; index: number }[]
  }

  export interface FoliateViewEvents {
    relocate: CustomEvent<FoliateLocation>
    load: CustomEvent<{ doc: Document; index: number }>
    "external-link": CustomEvent<{ a: HTMLAnchorElement; href: string }>
    link: CustomEvent<{ a: HTMLAnchorElement; href: string }>
  }

  export class View extends HTMLElement {
    book: FoliateBook
    renderer: FoliateRenderer
    isFixedLayout: boolean
    lastLocation: FoliateLocation | null
    history: EventTarget & { back(): void; forward(): void; readonly canGoBack: boolean; readonly canGoForward: boolean }
    open(book: BookFile | FoliateBook): Promise<void>
    close(): void
    init(options: { lastLocation?: string | number | null; showTextStart?: boolean }): Promise<void>
    goTo(target: string | number): Promise<unknown>
    goToFraction(fraction: number): Promise<void>
    goLeft(): Promise<void>
    goRight(): Promise<void>
    prev(distance?: number): Promise<void>
    next(distance?: number): Promise<void>
    getSectionFractions(): number[]
    addEventListener<K extends keyof FoliateViewEvents>(
      type: K,
      listener: (event: FoliateViewEvents[K]) => void,
      options?: AddEventListenerOptions,
    ): void
    addEventListener(type: string, listener: EventListenerOrEventListenerObject | null, options?: boolean | AddEventListenerOptions): void
  }

  export function makeBook(file: BookFile): Promise<FoliateBook>
}

declare module "foliate-js/comic-book.js" {
  import type { BookFile, FoliateBook } from "foliate-js/view.js"
  export interface ComicLoader {
    entries: { filename: string }[]
    loadBlob(name: string): Promise<Blob>
    getSize(name: string): number
  }
  export function makeComicBook(loader: ComicLoader, file: Pick<BookFile, "name">): FoliateBook
}

declare module "foliate-js/vendor/zip.js" {
  import type { BookFile } from "foliate-js/view.js"
  export interface ZipEntry {
    filename: string
    directory: boolean
    uncompressedSize: number
    getData?(writer: BlobWriter): Promise<Blob>
  }
  export class BlobReader {
    constructor(blob: BookFile)
  }
  export class BlobWriter {
    constructor(type?: string)
  }
  export class ZipReader {
    constructor(reader: BlobReader)
    getEntries(): Promise<ZipEntry[]>
    close(): Promise<void>
  }
  export function configure(options: { useWebWorkers: boolean }): void
}
