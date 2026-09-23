// Addresses the UI builds itself: image proxy URLs, provider artwork, and
// external title pages. None of them carry a token.

import type {
  ItemDetail,
  ItemSummary,
  Person,
  SeerrMediaDetail,
} from "./types.ts"

const POSTER_WIDTH = 400
/** Home progress cards are drawn wider and use 16:9 art. */
export const LANDSCAPE_WIDTH = 560
/** The detail hero renders edge to edge, so its backdrop needs a real width. */
export const BACKDROP_WIDTH = 1920
/** The detail hero's poster: a bigger slot than the library grid's. */
export const DETAIL_POSTER_WIDTH = 600
/** Episode still cards: wide grid slots, doubled to cover HiDPI. */
export const THUMBNAIL_WIDTH = 800
const HEADSHOT_WIDTH = 200
/** Title treatments are drawn at most ~28rem wide; twice that covers HiDPI. */
const LOGO_WIDTH = 800

type QueryParameterValue = string | number | boolean | null | undefined

export function queryString<T>(params: { [K in keyof T]: QueryParameterValue }) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === "") continue
    search.set(key, String(value))
  }
  const encoded = search.toString()
  return encoded ? `?${encoded}` : ""
}

/**
 * Poster/backdrop URL through the Rust image proxy, which keeps the token out
 * of the DOM. The parameter has to be spelled `maxWidth`: that is what the
 * proxy in `src/shell/cef/api.rs` reads and what it forwards to Jellyfin. Under
 * any other name the proxy sees no width at all and serves the untouched
 * original — 2000x3000 posters decoded into a 168px slot, which is what made
 * the grid stutter.
 */
export function imageUrl(
  item: Pick<ItemSummary, "id" | "primaryImageTag">,
  type: "Primary" | "Backdrop" | "Thumb" | "Logo" = "Primary",
  maxWidth = POSTER_WIDTH,
  // Every image type has its own tag. Passing the poster's tag along with
  // `Backdrop` asks Jellyfin for an image that does not exist under it, so the
  // caller has to say which tag goes with the type it asked for.
  tag: string | null = item.primaryImageTag,
) {
  return `/api/image/${encodeURIComponent(item.id)}/${type}${queryString({ maxWidth, tag })}`
}

/** Null when the item has no backdrop, so callers can lay out without one. */
export function backdropUrl(item: ItemDetail, maxWidth = BACKDROP_WIDTH) {
  if (!item.backdropImageTag) return null
  // Jellyfin supplies a series' backdrop tag as ParentBackdropImageTags on
  // seasons and episodes. That image still belongs to the series item: asking
  // the child id for the inherited tag produces a 404 from the image endpoint.
  const imageOwner =
    (item.kind === "Season" || item.kind === "Episode") && item.seriesId
      ? { id: item.seriesId, primaryImageTag: null }
      : item
  return imageUrl(imageOwner, "Backdrop", maxWidth, item.backdropImageTag)
}

/**
 * Landscape cards prefer an episode still, then Jellyfin's purpose-built
 * Thumb art, then a backdrop. A poster is the last-resort fallback so a title
 * with incomplete metadata still has something to show.
 */
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

/**
 * The item's own title treatment, or null where the server has none — which is
 * most of a typical library, so every caller has to keep its typeset heading as
 * the fallback rather than treating this as the primary path.
 *
 * An episode borrows nothing here on purpose: the logo that matters over an
 * episode still is the *show's*, and the caller knows its id.
 */
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

/** Cast headshots go through the same proxy: people are items in Jellyfin. */
export function personImageUrl(person: Person, maxWidth = HEADSHOT_WIDTH) {
  if (!person.id || !person.imageTag) return null
  return imageUrl({ id: person.id, primaryImageTag: person.imageTag }, "Primary", maxWidth)
}

// The first Companion-backed image proxy cached error JSON as immutable art.
// Keep the same disk key while forcing CEF to ask the repaired proxy once.
const SEERR_IMAGE_CACHE_VERSION = 2

/**
 * Provider art through the Desktop and Companion proxies. The UI supplies
 * only an allowlisted rendition name and provider-issued image path.
 */
export function seerrImageUrl(path: string | null | undefined, size = "w300") {
  if (!path) return null
  return `/api/seerr/image/${size}/${encodeURIComponent(path.replace(/^\//, ""))}?v=${SEERR_IMAGE_CACHE_VERSION}`
}

/**
 * External title pages the shell can open in the default browser. `source`
 * names the stable provider id each destination accepts. The kinds mirror the
 * native routes, so the UI never offers a link the shell would refuse to build.
 */
const EXTERNAL_PROVIDERS = [
  { id: "imdb", label: "IMDb", source: "imdb", kinds: ["Movie", "Series", "Episode"] },
  { id: "tmdb", label: "TMDB", source: "tmdb", kinds: ["Movie", "Series"] },
  { id: "tvdb", label: "TVDB", source: "tvdb", kinds: ["Movie", "Series", "Episode"] },
  { id: "letterboxd", label: "Letterboxd", source: "tmdb", kinds: ["Movie"] },
  { id: "trakt", label: "Trakt", source: "imdb", kinds: ["Movie", "Series"] },
] as const

export type ExternalProvider = (typeof EXTERNAL_PROVIDERS)[number]["id"]
type ExternalIdSource = (typeof EXTERNAL_PROVIDERS)[number]["source"]
type DiscoveryMediaKind = "Movie" | "Series"
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

function externalMediaUrl(
  provider: ExternalProvider,
  id: string | null | undefined,
  kind: DiscoveryMediaKind,
) {
  const definition = EXTERNAL_PROVIDERS.find((candidate) => candidate.id === provider)
  if (!definition || !validExternalId(definition.source, id)) return null

  switch (provider) {
    case "imdb":
      return `https://www.imdb.com/title/${id}/`
    case "tmdb":
      return `https://www.themoviedb.org/${kind === "Movie" ? "movie" : "tv"}/${id}`
    case "tvdb":
      return `https://thetvdb.com/dereferrer/${kind === "Movie" ? "movie" : "series"}/${id}`
    case "letterboxd":
      return `https://letterboxd.com/tmdb/${id}`
    case "trakt":
      return `https://trakt.tv/${kind === "Movie" ? "movies" : "shows"}/${id}`
  }
}

export function discoveryExternalLinksFor(
  item: Pick<SeerrMediaDetail, "mediaType" | "tmdbId" | "title" | "year" | "externalIds">,
): ExternalMediaLink[] {
  const kind = item.mediaType === "movie" ? "Movie" : "Series"
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
