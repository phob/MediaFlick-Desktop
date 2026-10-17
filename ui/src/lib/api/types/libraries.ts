import type { ItemSummary, SeriesStatus, SortOrder, WatchFilter } from "./catalog.ts"

export type LibraryKind =
  | "movies"
  | "shows"
  | "music"
  | "musicVideos"
  | "homeVideos"
  | "photos"
  | "books"
  | "boxSets"
  | "playlists"
  | "liveTv"
  | "trailers"
  | "mixed"

export type LibrarySortId =
  | "name"
  | "year"
  | "added"
  | "episode-added"
  | "rating"
  | "played"
  | "plays"
  | "runtime"
  | "random"

export type LibraryFilterId = "genre" | "decade" | "watched" | "parental" | "seriesStatus" | "favorite"

/** The sorts and filters a grid can run. A request for anything else answers 400. */
export interface BrowseCapabilities {
  sorts: LibrarySortId[]
  filters: LibraryFilterId[]
  letterIndex: boolean
}

export interface Library {
  id: string
  name: string
  kind: LibraryKind
  imageTag: string | null
  /** True while a movies or TV shows library's titles are not known yet: during the first catalog download, and until the library's first membership walk lands. A later walk keeps the stored titles, so it never sets this. */
  membershipPending: boolean
  browse: BrowseCapabilities
}

export interface LibrariesResponse {
  /** False until a sync cycle has read the server's library list. */
  known: boolean
  /** In the user's Jellyfin order. */
  libraries: Library[]
  crossLibrary: BrowseCapabilities
}

export type BrowsingPage = "last" | `library:${string}`

export interface LibraryQuery {
  genre?: string
  decade?: number
  watched?: WatchFilter | ""
  favorite?: boolean
  parental?: string
  seriesStatus?: SeriesStatus | ""
  sort?: LibrarySortId
  order?: SortOrder
  seed?: number
}

/**
 * A library's newest titles and the library sort "All" opens with: the shelf's
 * own order for catalog and newest-first libraries, date added for grouped ones.
 */
export interface LibraryLatest {
  items: ItemSummary[]
  sort: LibrarySortId
}

export interface LibraryFilterOptions {
  genres: string[]
  officialRatings: string[]
}
