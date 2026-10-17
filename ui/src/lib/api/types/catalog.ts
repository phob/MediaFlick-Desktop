export const CATALOG_KINDS = ["Movie", "Series", "Season", "Episode"] as const

export type CatalogKind = (typeof CATALOG_KINDS)[number]

type ItemKind = CatalogKind | (string & {})

export interface ItemSummary {
  id: string
  kind: ItemKind
  name: string
  year: number | null
  runtimeTicks: number | null
  communityRating: number | null
  officialRating: string | null
  seriesId: string | null
  seriesName: string | null
  indexNumber: number | null
  parentIndexNumber: number | null
  primaryImageTag: string | null
  thumbImageTag: string | null
  logoImageTag: string | null
  backdropImageTag: string | null
  childCount: number | null
  premiereDate: string | null
  seasonId: string | null
  played: boolean
  playCount: number
  positionTicks: number
  favorite: boolean
  unplayedCount: number | null
  /** Width over height of the primary image. Null for cached movies and shows, whose art is a 2:3 poster. */
  primaryAspect: number | null
  /**
   * `/api/item/{id}/children` carries this for a season's episodes, from its
   * live server reconcile, and a generic item's details carry their own. The
   * grid queries do not, and offline rows have none.
   */
  overview?: string | null
  /**
   * A playlist's children carry the slot each item fills, counted from zero.
   * The same item can fill two slots.
   */
  playlistIndex?: number
}

export interface Person {
  id: string | null
  name: string | null
  role: string | null
  type: string | null
  imageTag: string | null
}

export interface PersonIdentity {
  jellyfinId: string
  tmdbId: number | null
  name: string
  imageTag: string | null
}

export interface PersonResolution {
  person: PersonIdentity | null
  candidates: PersonIdentity[]
  ambiguous: boolean
}

export interface PersonResolveQuery {
  jellyfinId?: string
  tmdbId?: number
  name?: string
}

export interface ItemDetail extends ItemSummary {
  layout: "title"
  kind: CatalogKind
  genres: string[]
  originalTitle: string | null
  providerIds: { tmdb: string | null; imdb: string | null; tvdb: string | null }
  parentId: string | null
  dateCreated: string | null
  /** The library holding the title, or its series for a season or episode. Null until library membership is known. */
  libraryId: string | null
}

export interface GenericItemDetail extends ItemSummary {
  layout: "generic"
  genres: string[]
  isFolder: boolean
  dateCreated: string | null
}

export type AnyItemDetail = ItemDetail | GenericItemDetail

/**
 * Rich metadata fetched live from Jellyfin by `/api/item/{id}/about` for the
 * detail page. Nothing here is cached locally; with the server unreachable
 * these sections stay in their loading/error states.
 */
export interface ItemAbout {
  overview: string | null
  criticRating: number | null
  people: Person[]
  tags: string[]
  studios: string[]
}

export interface ItemSynopsis {
  overview: string | null
}

export type SortOrder = "asc" | "desc"

export type LibraryLetter =
  | "#" | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M"
  | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z"

export type LetterIndex = Partial<Record<LibraryLetter, number>>

export type WatchFilter = "unwatched" | "progress" | "watched"

export type SeriesStatus = "Continuing" | "Ended"

export interface ItemQuery {
  search?: string
  personId?: string
  kind?: string
  genre?: string
  /** Inclusive first year of a standard release decade (for example 1990). */
  decade?: number
  watched?: WatchFilter | ""
  favorite?: boolean
  parental?: string
  seriesStatus?: SeriesStatus | ""
  sort?: string
  order?: SortOrder
  seed?: number
  offset?: number
  limit?: number
}
