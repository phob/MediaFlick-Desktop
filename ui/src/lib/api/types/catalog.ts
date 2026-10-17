import type * as Wire from "../wire.gen.ts"

export const CATALOG_KINDS = ["Movie", "Series", "Season", "Episode"] as const

export type CatalogKind = (typeof CATALOG_KINDS)[number]

export type ItemSummary = Wire.ItemSummary

export type Person = Wire.Person

export type PersonIdentity = Wire.PersonIdentity

export type PersonResolution = Wire.PersonResolution

export interface PersonResolveQuery {
  jellyfinId?: string
  tmdbId?: number
  name?: string
}

export type ItemDetail = Extract<Wire.DetailView, { layout: "title" }>

export type GenericItemDetail = Extract<Wire.DetailView, { layout: "generic" }>

export type AnyItemDetail = Wire.DetailView

export type ItemAbout = Wire.ItemAbout

export type ItemSynopsis = Wire.ItemSynopsis

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
