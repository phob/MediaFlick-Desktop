import type { ItemDetail, ItemSummary, Person } from "./types/catalog.ts"
import type { SeerrMediaDetail } from "./types/seerr.ts"
import { link } from "./routes.ts"
import { seerrMediaType, titleKindOfSeerr, type TitleKind } from "../kinds.ts"

const POSTER_WIDTH = 400
export const LANDSCAPE_WIDTH = 560
export const BACKDROP_WIDTH = 1920
export const DETAIL_POSTER_WIDTH = 600
export const THUMBNAIL_WIDTH = 800
const HEADSHOT_WIDTH = 200
const LOGO_WIDTH = 800

export function imageUrl(
  item: Pick<ItemSummary, "id" | "primaryImageTag">,
  type: "Primary" | "Backdrop" | "Thumb" | "Logo" = "Primary",
  maxWidth = POSTER_WIDTH,
  tag: string | null = item.primaryImageTag,
) {
  return link("GET /api/image/{id}/{imageType}", {
    params: { id: item.id, imageType: type },
    query: { maxWidth, tag },
  })
}

export function backdropUrl(
  item: Pick<ItemSummary, "id" | "kind" | "seriesId" | "backdropImageTag">,
  maxWidth = BACKDROP_WIDTH,
) {
  if (!item.backdropImageTag) return null
  const ownerId =
    (item.kind === "Season" || item.kind === "Episode") && item.seriesId
      ? item.seriesId
      : item.id
  return imageUrl({ id: ownerId, primaryImageTag: null }, "Backdrop", maxWidth, item.backdropImageTag)
}

export function landscapeImageCandidates(
  item: Pick<
    ItemSummary,
    "id" | "kind" | "primaryImageTag" | "thumbImageTag" | "backdropImageTag"
  >,
  maxWidth = LANDSCAPE_WIDTH,
) {
  const candidates: string[] = []
  const add = (type: "Primary" | "Backdrop" | "Thumb", tag: string | null) => {
    if (tag) candidates.push(imageUrl(item, type, maxWidth, tag))
  }

  if (item.kind === "Episode") add("Primary", item.primaryImageTag)
  add("Thumb", item.thumbImageTag)
  add("Backdrop", item.backdropImageTag)
  if (item.kind !== "Episode") add("Primary", item.primaryImageTag)
  return [...new Set(candidates)]
}

export function logoUrl(
  item: Pick<ItemSummary, "id" | "logoImageTag">,
  maxWidth = LOGO_WIDTH,
) {
  if (!item.logoImageTag) return null
  return imageUrl(
    { id: item.id, primaryImageTag: null },
    "Logo",
    maxWidth,
    item.logoImageTag,
  )
}

export function personImageUrl(person: Person, maxWidth = HEADSHOT_WIDTH) {
  if (!person.id || !person.imageTag) return null
  return imageUrl({ id: person.id, primaryImageTag: person.imageTag }, "Primary", maxWidth)
}

export function seerrImageUrl(path: string | null | undefined, size = "w300") {
  if (!path) return null
  return link("GET /api/seerr/image/{size}/{file}", {
    params: { size, file: path.replace(/^\//, "") },
  })
}

const EXTERNAL_PROVIDERS = [
  { id: "imdb", label: "IMDb", source: "imdb", kinds: ["Movie", "Series", "Episode"] },
  { id: "tmdb", label: "TMDB", source: "tmdb", kinds: ["Movie", "Series"] },
  { id: "tvdb", label: "TVDB", source: "tvdb", kinds: ["Movie", "Series", "Episode"] },
  { id: "letterboxd", label: "Letterboxd", source: "tmdb", kinds: ["Movie"] },
  { id: "trakt", label: "Trakt", source: "imdb", kinds: ["Movie", "Series"] },
] as const

export type ExternalProvider = (typeof EXTERNAL_PROVIDERS)[number]["id"]
type ExternalIdSource = (typeof EXTERNAL_PROVIDERS)[number]["source"]
type ExternalLinkId = ExternalProvider | "rotten-tomatoes-search"

export interface ExternalMediaLink {
  id: ExternalLinkId
  label: string
  href: string
  actionLabel?: string
}

export type ExternalMenuLink =
  | ExternalMediaLink
  | { id: ExternalProvider; label: string }

function validExternalId(source: ExternalIdSource, value: string | null | undefined) {
  if (!value || value.length > 32) return false
  if (source === "imdb") return /^tt\d+$/.test(value)
  return /^\d+$/.test(value) && Number(value) > 0
}

function rottenTomatoesSearchLink(title: string, year: number | null): ExternalMediaLink | null {
  const normalizedTitle = title.trim()
  if (!normalizedTitle) return null

  const normalizedYear = Number.isInteger(year) && year !== null && year > 1800
    ? ` ${year}`
    : ""
  const query = encodeURIComponent(`${normalizedTitle}${normalizedYear}`)
  return {
    id: "rotten-tomatoes-search",
    label: "Rotten Tomatoes",
    actionLabel: "Search Rotten Tomatoes",
    href: `https://www.rottentomatoes.com/search?search=${query}`,
  }
}

export function externalLinksFor(
  item: Pick<ItemDetail, "kind" | "name" | "year" | "providerIds">,
): ExternalMenuLink[] {
  const exactLinks = EXTERNAL_PROVIDERS.filter(
    (provider) =>
      validExternalId(provider.source, item.providerIds?.[provider.source]) &&
      provider.kinds.some((kind) => kind === item.kind),
  )
  const rottenTomatoes = item.kind === "Movie" || item.kind === "Series"
    ? rottenTomatoesSearchLink(item.name, item.year)
    : null
  return rottenTomatoes ? [...exactLinks, rottenTomatoes] : exactLinks
}

const TVDB_PATHS = { Movie: "movie", Series: "series" } satisfies Record<TitleKind, string>

function externalMediaUrl(
  provider: ExternalProvider,
  id: string | null | undefined,
  kind: TitleKind,
) {
  const definition = EXTERNAL_PROVIDERS.find((candidate) => candidate.id === provider)
  if (!definition || !validExternalId(definition.source, id)) return null

  switch (provider) {
    case "imdb":
      return `https://www.imdb.com/title/${id}/`
    case "tmdb":
      return `https://www.themoviedb.org/${seerrMediaType(kind)}/${id}`
    case "tvdb":
      return `https://thetvdb.com/dereferrer/${TVDB_PATHS[kind]}/${id}`
    case "letterboxd":
      return `https://letterboxd.com/tmdb/${id}`
    case "trakt":
      return `https://trakt.tv/${kind === "Movie" ? "movies" : "shows"}/${id}`
  }
}

export function discoveryExternalLinksFor(
  item: Pick<SeerrMediaDetail, "mediaType" | "tmdbId" | "title" | "year" | "externalIds">,
): ExternalMediaLink[] {
  const kind = titleKindOfSeerr(item.mediaType)
  const externalIds = item.externalIds ?? { imdb: null, tvdb: null }
  const ids = {
    imdb: externalIds.imdb,
    tmdb: String(item.tmdbId),
    tvdb: externalIds.tvdb === null ? null : String(externalIds.tvdb),
  } satisfies Record<ExternalIdSource, string | null>

  const exactLinks = EXTERNAL_PROVIDERS.flatMap((provider) => {
    if (!provider.kinds.some((candidate) => candidate === kind)) return []
    const href = externalMediaUrl(provider.id, ids[provider.source], kind)
    return href ? [{ id: provider.id, label: provider.label, href }] : []
  })
  const rottenTomatoes = rottenTomatoesSearchLink(item.title, item.year)
  return rottenTomatoes ? [...exactLinks, rottenTomatoes] : exactLinks
}
