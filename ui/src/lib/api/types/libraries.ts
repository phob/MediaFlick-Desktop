import type * as Wire from "../wire.gen.ts"
import type { SeriesStatus, SortOrder, WatchFilter } from "./catalog.ts"

export type LibraryKind = Wire.LibraryKind

export type LibrarySortId = Wire.SortKey

export type LibraryFilterId = Wire.FilterKey

export type BrowseCapabilities = Wire.BrowseCapabilities

export type Library = Wire.LibraryView

export type LibrariesResponse = Wire.LibrariesResponse

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

export type LibraryLatest = Wire.LibraryLatest
