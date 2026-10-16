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
  type StartupDestination,
} from "./api/types.ts"

export type BrowseSource =
  | { scope: "titles"; query: ItemQuery }
  | { scope: "library"; libraryId: string; query: LibraryQuery }
  | { scope: "children"; itemId: string }

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

export function startupRoute(destination: StartupDestination, last: string | undefined) {
  if (typeof destination === "object") return libraryPath(destination.library)
  switch (destination) {
    case "home": return "/"
    case "calendar": return "/calendar"
    case "last": return last ?? "/"
  }
}

export function genreRoute(genre: string, item: ItemDetail) {
  const params = new URLSearchParams({ genre })
  return item.libraryId ? libraryPath(item.libraryId, params) : `/library?${params}`
}
