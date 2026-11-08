import type { FoliateBook, FoliateLocation, FoliateTocItem, View } from "foliate-js/view.js"
import type { BookFormat, BookManifest, ReaderAppearance, ReadingLocation } from "@/lib/api"
import { installAppSurfaceGuard } from "@/lib/app-surface"
import { bookStyles, fitAttribute, layoutAttributes, themeColors } from "./appearance"
import { archiveComic, zipComic } from "./comics"
import { foliateComics, foliateView } from "./engines"
import type { ReaderCommand } from "./model"
import { RemoteFile } from "./remote-file"

/** Formats whose position Jellyfin keeps as a fraction of the text; the others keep pages. */
const TEXT_FORMATS: ReadonlySet<BookFormat> = new Set(["epub", "mobi", "azw", "azw3"])
const COMIC_FORMATS: ReadonlySet<BookFormat> = new Set(["cbz", "cbr", "cb7", "cbt"])
/** Keys playback owns. A section document keeps them from the app, so they are passed on. */
const PLAYBACK_KEYS: ReadonlySet<string> = new Set(["MediaPlayPause", "MediaPlay", "MediaPause", "MediaStop"])
const PAGE_STEP = 10

export interface TocEntry {
  label: string
  target: string | number
  depth: number
}

export interface ReadingPosition {
  location: ReadingLocation
  finished: boolean
  fraction: number
  /** Zero-based page and page count of a book with fixed pages: a PDF, a comic or a fixed-layout EPUB. */
  page: { current: number; total: number } | null
  /** Zero-based location (about a screen of text) and their count in a text book. */
  textLocation: { current: number; total: number } | null
  chapter: string | null
  /** The contents entry the reader is in: its href, or for a page list the page. */
  tocTarget: string | number | null
  canReturn: boolean
}

export type JumpTarget = { kind: "toc"; target: string | number } | { kind: "fraction"; fraction: number } | { kind: "page"; page: number }

type SessionCommand = Extract<ReaderCommand, { kind: "turn" | "scroll" | "section" | "edge" | "return" }>

export interface BookSession {
  readonly layout: "reflowable" | "fixed"
  readonly toc: readonly TocEntry[]
  position(): ReadingPosition | null
  run(command: SessionCommand): Promise<void>
  jump(target: JumpTarget): Promise<void>
  apply(appearance: ReaderAppearance): void
  close(): void
}

export interface OpenOptions {
  signal: AbortSignal
  /** Every key pressed inside the book's documents, which the page's own listeners never see. */
  onKey(event: KeyboardEvent): void
  onPosition(position: ReadingPosition): void
  onDownload(fraction: number): void
}

async function loadBook(file: RemoteFile, format: BookFormat, onDownload: (fraction: number) => void): Promise<FoliateBook> {
  if (COMIC_FORMATS.has(format)) {
    const { makeComicBook } = await foliateComics()
    const loader = format === "cbz" ? await zipComic(file) : await archiveComic(file, onDownload)
    return makeComicBook(loader, file)
  }
  const { makeBook } = await foliateView()
  return makeBook(file)
}

function flatten(items: readonly FoliateTocItem[], depth = 0): TocEntry[] {
  return items.flatMap((item) => [{ label: item.label.trim(), target: item.href, depth }, ...flatten(item.subitems ?? [], depth + 1)])
}

function pageEntries(count: number): TocEntry[] {
  return Array.from({ length: count }, (_, index) => ({ label: `Page ${index + 1}`, target: index, depth: 0 }))
}

export async function openBook(host: HTMLElement, itemId: string, manifest: BookManifest, options: OpenOptions): Promise<BookSession> {
  const { View } = await foliateView()
  // foliate tells a CBZ from an EPUB by the name, so the name carries the format the bytes were sniffed as.
  const file = await RemoteFile.open(itemId, `${itemId}.${manifest.format}`, options.signal)
  const book = await loadBook(file, manifest.format, options.onDownload)
  const view: View = new View()
  // Until it is destroyed a book keeps its blob URLs, and a PDF its pdf.js worker.
  const close = () => {
    view.close()
    view.remove()
    book.destroy?.()
  }
  try {
    options.signal.throwIfAborted()
    const comic = COMIC_FORMATS.has(manifest.format)
    if (comic || manifest.format === "pdf") book.rendition = { ...book.rendition, spread: "none" }

    view.style.cssText = "display: block; width: 100%; height: 100%"
    let restored = false
    const toPosition = (detail: FoliateLocation): ReadingPosition => {
      const section = detail.section ?? { current: 0, total: book.sections.length }
      // foliate weighs fixed pages by their bytes, which makes a comic's percent jump; a page is a page here.
      const fraction = view.isFixedLayout
        ? section.current / Math.max(1, section.total - 1)
        : Math.min(1, Math.max(0, detail.fraction ?? 0))
      const text = TEXT_FORMATS.has(manifest.format)
      return {
        location: text
          ? { kind: "text", cfi: detail.cfi, fraction }
          : { kind: "page", page: section.current, pages: Math.max(section.total, section.current + 1) },
        finished: view.isFixedLayout ? section.current >= section.total - 1 : Boolean(view.renderer.atEnd),
        fraction,
        page: text && !view.isFixedLayout ? null : { current: section.current, total: section.total },
        textLocation: text && detail.location ? { current: detail.location.current, total: detail.location.total } : null,
        chapter: comic ? null : (detail.tocItem?.label?.trim() ?? null),
        tocTarget: text ? (detail.tocItem?.href ?? null) : section.current,
        canReturn: view.history.canGoBack,
      }
    }
    view.addEventListener("load", ({ detail }) => {
      installAppSurfaceGuard(detail.doc)
      detail.doc.addEventListener(
        "keydown",
        (event) => {
          if (PLAYBACK_KEYS.has(event.key)) window.dispatchEvent(new KeyboardEvent(event.type, event))
          else options.onKey(event)
        },
        { capture: true, signal: options.signal },
      )
    })
    view.addEventListener("relocate", ({ detail }) => {
      if (restored) options.onPosition(toPosition(detail))
    })
    view.addEventListener("external-link", (event) => event.preventDefault())
    host.replaceChildren(view)
    await view.open(book)

    const layout = view.isFixedLayout ? "fixed" : "reflowable"
    const sections = book.sections.length
    const realToc = comic ? [] : flatten(book.toc ?? [])
    const toc = realToc.length || layout === "reflowable" ? realToc : pageEntries(sections)
    let chapterPages: Promise<number[]> | null = null

    const apply = (appearance: ReaderAppearance) => {
      host.style.background = themeColors(appearance.theme).paper
      if (layout === "fixed") {
        view.renderer.setAttribute("zoom", fitAttribute(appearance))
        return
      }
      for (const [name, value] of Object.entries(layoutAttributes(appearance))) view.renderer.setAttribute(name, value)
      view.renderer.setStyles?.(bookStyles(appearance))
    }
    apply(manifest.appearance)

    const resume = manifest.resume
    if (resume.kind === "exact") {
      await view.init({ lastLocation: resume.location.kind === "text" ? resume.location.cfi : resume.location.page })
    } else if (resume.kind === "page") {
      await view.init({ lastLocation: resume.page })
    } else {
      await view.init({})
      if (resume.kind === "fraction") await view.goToFraction(resume.fraction)
    }
    options.signal.throwIfAborted()
    restored = true

    const currentPage = () => view.lastLocation?.section?.current ?? 0
    const sectionJump = async (direction: 1 | -1) => {
      if (layout === "reflowable") return direction > 0 ? view.renderer.nextSection() : view.renderer.prevSection()
      const page = currentPage()
      if (realToc.length && book.resolveHref) {
        const resolveHref = book.resolveHref.bind(book)
        chapterPages ??= Promise.all(realToc.map(async (entry) => (await resolveHref(String(entry.target))).index))
        const starts = [...new Set(await chapterPages)].sort((a, b) => a - b)
        const target = direction > 0 ? starts.find((start) => start > page) : starts.findLast((start) => start < page)
        if (target !== undefined) return void (await view.goTo(target))
      }
      await view.goTo(Math.min(sections - 1, Math.max(0, page + direction * PAGE_STEP)))
    }

    const scrollOrTurn = async (to: "next" | "prev") => {
      const page = view.renderer
      const step = page.clientHeight * 0.85
      const atEdge = to === "next" ? page.scrollTop + page.clientHeight >= page.scrollHeight - 1 : page.scrollTop <= 0
      if (!atEdge) return page.scrollBy({ top: to === "next" ? step : -step })
      await (to === "next" ? view.next() : view.prev())
      page.scrollTop = to === "next" ? 0 : page.scrollHeight
    }

    return {
      layout,
      toc,
      position: () => (view.lastLocation ? toPosition(view.lastLocation) : null),
      async run(command) {
        switch (command.kind) {
          case "turn":
            if (command.to === "left") return view.goLeft()
            if (command.to === "right") return view.goRight()
            return layout === "fixed" ? scrollOrTurn(command.to) : command.to === "next" ? view.next() : view.prev()
          case "scroll":
            return command.by > 0 ? view.next(host.clientHeight / 8) : view.prev(host.clientHeight / 8)
          case "section":
            return sectionJump(command.to === "next" ? 1 : -1)
          case "edge":
            return view.goToFraction(command.to === "start" ? 0 : 1)
          case "return":
            return view.history.back()
        }
      },
      async jump(target) {
        if (target.kind === "fraction") await view.goToFraction(target.fraction)
        else await view.goTo(target.kind === "toc" ? target.target : target.page)
      },
      apply,
      close,
    }
  } catch (error) {
    close()
    throw error
  }
}
