import type { ItemSummary, Person } from "./types/catalog.ts"
import { link } from "./routes.ts"

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

export function trickplayTileUrl(
  itemId: string,
  width: number,
  tileIndex: number,
  mediaSourceId?: string | null,
) {
  return link("GET /api/trickplay/{id}/{width}/{tile}", {
    params: { id: itemId, width, tile: tileIndex },
    query: { mediaSourceId },
  })
}

export function seerrImageUrl(path: string | null | undefined, size = "w300") {
  if (!path) return null
  return link("GET /api/seerr/image/{size}/{file}", {
    params: { size, file: path.replace(/^\//, "") },
  })
}
