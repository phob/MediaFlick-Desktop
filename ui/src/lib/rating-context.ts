import { createContext, useContext, useEffect } from "react"
import type {
  ItemRatings,
  NormalizedRating,
  RatingSourceDefinition,
} from "./api"

export type RatingsContextValue = {
  items: ReadonlyMap<string, ItemRatings>
  selected: readonly string[]
  definitions: ReadonlyMap<string, RatingSourceDefinition>
  register: (id: string) => () => void
}

export const RatingsContext = createContext<RatingsContextValue | null>(null)

export type DisplayRating = {
  rating: NormalizedRating
  definition: RatingSourceDefinition
  formatted: string
  accessibleValue: string
}

interface CardRatingsResult {
  ratings: DisplayRating[]
  item: ItemRatings | undefined
}

export function formatRating(
  rating: NormalizedRating,
  definition: RatingSourceDefinition,
): Pick<DisplayRating, "formatted" | "accessibleValue"> {
  const value = Number(rating.value)
  if (!Number.isFinite(value)) return { formatted: "", accessibleValue: "" }
  switch (definition.format) {
    case "percent": {
      const rounded = Math.round(value)
      return { formatted: `${rounded}%`, accessibleValue: `${rounded} percent` }
    }
    case "integer": {
      const rounded = Math.round(value)
      return {
        formatted: String(rounded),
        accessibleValue: `${rounded} out of ${definition.scaleMax}`,
      }
    }
    case "stars": {
      const decimal = value.toFixed(1)
      return {
        formatted: `★${decimal}`,
        accessibleValue: `${decimal} out of ${definition.scaleMax}`,
      }
    }
    default: {
      const decimal = value.toFixed(1)
      return {
        formatted: decimal,
        accessibleValue: `${decimal} out of ${definition.scaleMax}`,
      }
    }
  }
}

export function useCardRatings(itemId: string): CardRatingsResult {
  const context = useContext(RatingsContext)
  useEffect(() => context?.register(itemId), [context, itemId])
  if (!context) return { ratings: [], item: undefined }
  const item = context.items.get(itemId)
  const bySource = new Map(item?.ratings.map((rating) => [rating.sourceId, rating]) ?? [])
  const ratings = context.selected.flatMap((sourceId) => {
    const rating = bySource.get(sourceId)
    const definition = context.definitions.get(sourceId)
    if (!rating || !definition) return []
    const formatted = formatRating(rating, definition)
    if (!formatted.formatted) return []
    return [{ rating, definition, ...formatted }]
  })
  return { ratings, item }
}
