import type { CalendarEntry } from "./api/types/calendar.ts"
import type { CatalogKind } from "./api/types/catalog.ts"
import type { NormalizedCollectionTitle } from "./api/types/collections.ts"
import type { SeerrMediaType } from "./api/types/seerr.ts"

/**
 * Every translation between the kind vocabularies lives here: Jellyfin's
 * `Movie`/`Series`, Seerr's (and TMDB's) `movie`/`tv`, collection titles'
 * `movie`/`series`, and calendar entries' `movie`/`episode`. The
 * `ui/no-inline-kind-translation` lint rule rejects a ternary that does it
 * anywhere else.
 */

/** The kinds a title listing holds: Jellyfin's top-level `Movie` and `Series`. */
export type TitleKind = Extract<CatalogKind, "Movie" | "Series">

export function isTitleKind(value: string): value is TitleKind {
  return value === "Movie" || value === "Series"
}

/** Seerr's word for a title kind, which is TMDB's too. */
export function seerrMediaType(kind: TitleKind): SeerrMediaType {
  return kind === "Movie" ? "movie" : "tv"
}

export function titleKindOfSeerr(mediaType: SeerrMediaType): TitleKind {
  return mediaType === "movie" ? "Movie" : "Series"
}

export function titleKindOfCollection(mediaType: NormalizedCollectionTitle["mediaType"]): TitleKind {
  return mediaType === "series" ? "Series" : "Movie"
}

/** An upcoming episode belongs to a series, so it is requested as one. */
export function titleKindOfCalendar(kind: CalendarEntry["kind"]): TitleKind {
  return kind === "movie" ? "Movie" : "Series"
}
