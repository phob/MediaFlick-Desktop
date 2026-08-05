import type { ItemRatings, ItemSummary } from "@/lib/api"
import { useCardRatings, type DisplayRating } from "@/lib/rating-context"

type RatingReadoutViewProps = {
  itemName: string
  ratings: DisplayRating[]
  ratingItem: ItemRatings | undefined
}

function RatingReadoutView({
  itemName,
  ratings,
  ratingItem,
  variant,
  hasRibbon = false,
}: RatingReadoutViewProps & {
  variant: "card" | "detail"
  hasRibbon?: boolean
}) {
  if (!ratingItem || ratings.length === 0) return null
  const originLabel = ratingItem.origin === "local_mdblist" ? "local MDBList" : "MediaFlick server plugin"
  return (
    <dl
      className={variant === "card" ? "card-rating-readout" : "detail-rating-readout"}
      aria-label={`Ratings for ${itemName}`}
      data-rating-origin={ratingItem.origin}
      data-stale={ratingItem.stale || undefined}
      data-ribbon={variant === "card" && hasRibbon ? true : undefined}
    >
      {ratings.map(({ rating, definition, formatted, accessibleValue }) => (
        <div
          key={rating.sourceId}
          className="rating-readout-value"
          title={`${definition.label}: ${accessibleValue} · via ${originLabel}${ratingItem.stale ? " · cached" : ""}`}
        >
          <dt className="sr-only">{definition.label}</dt>
          <dd aria-label={`${definition.label} rating ${accessibleValue}`}>
            <span className="rating-readout-source" aria-hidden>{definition.shortLabel}</span>
            <span>{formatted}</span>
          </dd>
        </div>
      ))}
    </dl>
  )
}

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

/** Selected MDBList sources beside the rest of a title's detail metadata. */
export function DetailRatingReadout({ item }: { item: Pick<ItemSummary, "id" | "name"> }) {
  const { ratings, item: ratingItem } = useCardRatings(item.id)
  return (
    <RatingReadoutView
      itemName={item.name}
      ratings={ratings}
      ratingItem={ratingItem}
      variant="detail"
    />
  )
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
  return (
    <RatingReadoutView
      itemName={itemName}
      ratings={ratings}
      ratingItem={ratingItem}
      variant="card"
      hasRibbon={hasRibbon}
    />
  )
}
