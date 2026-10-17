import type * as Wire from "../wire.gen.ts"

export const CATALOG_KINDS = ["Movie", "Series", "Season", "Episode"] as const

export type CatalogKind = (typeof CATALOG_KINDS)[number]

export type ItemSummary = Wire.ItemSummary

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

export type ItemDetail = Extract<Wire.DetailView, { layout: "title" }>

export type GenericItemDetail = Extract<Wire.DetailView, { layout: "generic" }>

export type AnyItemDetail = Wire.DetailView

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
