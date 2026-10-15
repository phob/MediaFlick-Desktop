import {
  BookOpen,
  Clapperboard,
  Film,
  Folder,
  Images,
  Layers,
  ListMusic,
  Music,
  Radio,
  Tv,
  Video,
  type LucideIcon,
} from "lucide-react"
import {
  CATALOG_KINDS,
  type BrowsingPage,
  type CatalogKind,
  type ItemDetail,
  type ItemQuery,
  type Library,
  type LibraryKind,
  type LibraryQuery,
} from "./api/types.ts"

export type BrowseSource =
  | { scope: "titles"; query: ItemQuery }
  | { scope: "library"; libraryId: string; query: LibraryQuery }
  | { scope: "children"; itemId: string }

/** A grid the user sorts and filters: a library, or titles across libraries. */
export type ListingSource = Exclude<BrowseSource, { scope: "children" }>

const catalogKinds: ReadonlySet<string> = new Set(CATALOG_KINDS)

export function isCatalogKind(kind: string): kind is CatalogKind {
  return catalogKinds.has(kind)
}

export function libraryPath(id: string, params?: URLSearchParams) {
  const search = params?.toString()
  return `/library/${encodeURIComponent(id)}${search ? `?${search}` : ""}`
}

export function libraryIdFromPath(pathname: string): string | null {
  const match = pathname.match(/^\/library\/([^/]+)$/)
  if (!match) return null
  try {
    return decodeURIComponent(match[1])
  } catch {
    return null
  }
}

const LIBRARY_ICONS: Record<LibraryKind, LucideIcon> = {
  movies: Film,
  shows: Tv,
  music: Music,
  musicVideos: Clapperboard,
  homeVideos: Video,
  photos: Images,
  books: BookOpen,
  boxSets: Layers,
  playlists: ListMusic,
  liveTv: Radio,
  trailers: Clapperboard,
  mixed: Folder,
}

export function libraryIcon(kind: LibraryKind): LucideIcon {
  return LIBRARY_ICONS[kind]
}

export function browsingKey(libraryId: string): BrowsingPage {
  return `library:${libraryId}`
}

export function firstLibraryOf(libraries: readonly Library[], kind: LibraryKind) {
  return libraries.find((library) => library.kind === kind) ?? null
}

/** A title page's genre chip: its own library when that library filters by genre, else every title. */
export function genreRoute(genre: string, item: ItemDetail, libraries: readonly Library[]) {
  const params = new URLSearchParams({ genre })
  const library = libraries.find((candidate) => candidate.id === item.libraryId)
  return library?.browse.filters.includes("genre")
    ? libraryPath(library.id, params)
    : `/library?${params}`
}
