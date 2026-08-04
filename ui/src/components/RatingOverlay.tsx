import type { ItemRatings, ItemSummary } from "@/lib/api"
import { useCardRatings, type DisplayRating } from "@/lib/rating-context"

/** Compact multi-source card ratings; absent sources render nothing at all. */
export function RatingOverlay({ item }: { item: Pick<ItemSummary, "id" | "name"> }) {
  const { ratings, item: ratingItem } = useCardRatings(item.id)
  return <RatingOverlayView itemName={item.name} ratings={ratings} ratingItem={ratingItem} />
}

export function RatingOverlayView({
  itemName,
  ratings,
  ratingItem,
}: {
  itemName: string
  ratings: DisplayRating[]
  ratingItem: ItemRatings | undefined
}) {
  if (!ratingItem || ratings.length === 0) return null
  const originLabel = ratingItem.origin === "local_mdblist" ? "local MDBList" : "MediaFlick server plugin"
  return (
    <dl
      className="card-rating-overlay"
      aria-label={`Ratings for ${itemName}`}
      data-rating-origin={ratingItem.origin}
      data-stale={ratingItem.stale || undefined}
    >
      {ratings.map(({ rating, definition, formatted, accessibleValue }) => (
        <div
          key={rating.sourceId}
          className="card-rating-chip"
          title={`${definition.label}: ${accessibleValue} · via ${originLabel}${ratingItem.stale ? " · cached" : ""}`}
        >
          <dt aria-label={definition.label}>{definition.shortLabel}</dt>
          <dd aria-label={`${definition.label} rating ${accessibleValue}`}>{formatted}</dd>
        </div>
      ))}
    </dl>
  )
}
