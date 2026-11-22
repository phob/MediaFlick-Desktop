import type { CalendarEntry } from "./api/types/calendar.ts"
import type { CatalogKind } from "./api/types/catalog.ts"
import type { NormalizedCollectionTitle } from "./api/types/collections.ts"
import type { SeerrMediaType } from "./api/types/seerr.ts"

export type TitleKind = Extract<CatalogKind, "Movie" | "Series">

export function isTitleKind(value: string): value is TitleKind {
  return value === "Movie" || value === "Series"
}

export function seerrMediaType(kind: TitleKind): SeerrMediaType {
  return kind === "Movie" ? "movie" : "tv"
}

export function titleKindOfSeerr(mediaType: SeerrMediaType): TitleKind {
  return mediaType === "movie" ? "Movie" : "Series"
}

export function titleKindOfCollection(mediaType: NormalizedCollectionTitle["mediaType"]): TitleKind {
  return mediaType === "series" ? "Series" : "Movie"
}

export function titleKindOfCalendar(kind: CalendarEntry["kind"]): TitleKind {
  return kind === "movie" ? "Movie" : "Series"
}
