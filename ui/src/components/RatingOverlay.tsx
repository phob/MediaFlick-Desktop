import { Star } from "lucide-react"
import type { ItemRatings, ItemSummary } from "@/lib/api"
import { useCardRatings, type DisplayRating } from "@/lib/rating-context"

/** Compact multi-source card ratings; absent sources render nothing at all. */
export function RatingOverlay({
  item,
  hasRibbon = false,
}: {
  item: Pick<ItemSummary, "id" | "name">
  hasRibbon?: boolean
}) {
  const { ratings, item: ratingItem } = useCardRatings(item.id)
  return <RatingOverlayView
    itemName={item.name}
    ratings={ratings}
    ratingItem={ratingItem}
    hasRibbon={hasRibbon}
  />
}

export function RatingOverlayView({
  itemName,
  ratings,
  ratingItem,
  hasRibbon = false,
}: {
  itemName: string
  ratings: DisplayRating[]
  ratingItem: ItemRatings | undefined
  hasRibbon?: boolean
}) {
  if (!ratingItem || ratings.length === 0) return null
  const originLabel = ratingItem.origin === "local_mdblist" ? "local MDBList" : "MediaFlick server plugin"
  return (
    <dl
      className="card-rating-readout"
      aria-label={`Ratings for ${itemName}`}
      data-rating-origin={ratingItem.origin}
      data-stale={ratingItem.stale || undefined}
      data-ribbon={hasRibbon || undefined}
    >
      <div className="card-rating-row">
        <Star aria-hidden />
        <div className="card-rating-values">
          {ratings.map(({ rating, definition, formatted, accessibleValue }, index) => (
            <div
              key={rating.sourceId}
              className="card-rating-value"
              title={`${definition.label}: ${accessibleValue} · via ${originLabel}${ratingItem.stale ? " · cached" : ""}`}
            >
              {index > 0 && <span className="card-rating-separator" aria-hidden>·</span>}
              <dt className="sr-only">{definition.label}</dt>
              <dd aria-label={`${definition.label} rating ${accessibleValue}`}>
                <span className="card-rating-source" aria-hidden>{definition.shortLabel}</span>
                <span>{formatted}</span>
              </dd>
            </div>
          ))}
        </div>
      </div>
    </dl>
  )
}
