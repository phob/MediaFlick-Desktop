import type { ItemDetail, ItemSummary } from "../../src/lib/api"
import { isJsonObject, type JsonObject, type JsonValue } from "../../src/lib/json"

export function itemSummary(
  overrides: Pick<ItemSummary, "id" | "kind" | "name"> & Partial<ItemSummary>,
): ItemSummary {
  const { id, kind, name, ...rest } = overrides
  return {
    id,
    kind,
    name,
    year: null,
    runtimeTicks: null,
    communityRating: null,
    officialRating: null,
    seriesId: null,
    seriesName: null,
    indexNumber: null,
    parentIndexNumber: null,
    primaryImageTag: null,
    thumbImageTag: null,
    logoImageTag: null,
    backdropImageTag: null,
    childCount: null,
    premiereDate: null,
    seasonId: null,
    played: false,
    playCount: 0,
    positionTicks: 0,
    favorite: false,
    ...rest,
  }
}

export function itemDetail(
  overrides: Pick<ItemDetail, "id" | "kind" | "name"> & Partial<ItemDetail>,
): ItemDetail {
  return {
    ...itemSummary(overrides),
    genres: [],
    originalTitle: null,
    providerIds: { tmdb: null, imdb: null, tvdb: null },
    parentId: null,
    dateCreated: null,
    ...overrides,
  }
}

export function requireElement<ElementType extends Element>(
  element: ElementType | null,
  description: string,
): ElementType {
  if (element === null) throw new Error(`Expected ${description}`)
  return element
}

export function parseJsonObject(text: string): JsonObject {
  const value: JsonValue = JSON.parse(text)
  if (!isJsonObject(value)) throw new Error("Expected a JSON object")
  return value
}
