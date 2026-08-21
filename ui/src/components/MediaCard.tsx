import { memo, useEffect, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { CardInlineActions } from "@/components/CardInlineActions"
import { CardTechnicalReadout } from "@/components/CardTechnicalReadout"
import { RatingOverlay } from "@/components/RatingOverlay"
import {
  imageUrl,
  landscapeImageCandidates,
  progressFraction,
  type ItemSummary,
} from "@/lib/api"
import { formatRemaining } from "@/lib/format"
import { usePreview } from "@/lib/preview"
import { detailNavigationState } from "@/lib/navigation"
import { cn } from "@/lib/utils"

const TECHNICAL_PREFETCH_MARGIN = "160px"

/**
 * Horizontal shelves mount every card so they can scroll smoothly, including
 * cards far outside both the viewport and a shelf's clipped edge. Observe the
 * actual link before registering its live technical metadata; a small margin
 * lets badges arrive just before a card scrolls into view.
 */
function useTechnicalVisibility() {
  const intersectionSupported = globalThis.IntersectionObserver !== undefined
  const [element, setElement] = useState<HTMLElement | null>(null)
  const [visible, setVisible] = useState(!intersectionSupported)

  useEffect(() => {
    if (!element || !intersectionSupported) return
    const observer = new IntersectionObserver(
      (entries) => setVisible(entries.some((entry) => entry.isIntersecting)),
      { rootMargin: TECHNICAL_PREFETCH_MARGIN },
    )
    observer.observe(element)
    return () => observer.disconnect()
  }, [element, intersectionSupported])

  return { observe: setElement, visible }
}

function subtitleFor(item: ItemSummary) {
  if (item.kind === "Episode") {
    const season = item.parentIndexNumber
    const episode = item.indexNumber
    const code = season != null && episode != null ? `S${season}E${episode}` : null
    return [item.seriesName, code].filter(Boolean).join(" · ")
  }
  if (item.kind === "Series" && item.childCount) {
    return `${item.childCount} ${item.childCount === 1 ? "season" : "seasons"}`
  }
  return item.year ? String(item.year) : ""
}

/**
 * Memoised because the windowed grid re-renders on every scroll tick: without
 * it a full screen of cards reconciles ~60 times a second for nothing. `item`
 * comes straight out of the query cache, whose structural sharing keeps the
 * reference stable between renders, so the comparison actually holds.
 */
export const MediaCard = memo(function MediaCard({
  item,
  className,
  landscape = false,
  ribbon,
  preview = true,
}: {
  item: ItemSummary
  className?: string
  landscape?: boolean
  /** A corner flag — "Top 10", "New" — drawn over the artwork. */
  ribbon?: string
  /** Off for cards the expanded panel would sit badly on, such as a Top 10 rank. */
  preview?: boolean
}) {
  const location = useLocation()
  const { handlers, expanded, previewsEnabled } = usePreview(item, preview)
  const { observe: observeTechnical, visible: technicalVisible } = useTechnicalVisibility()
  const progress = progressFraction(item)
  const subtitle = subtitleFor(item)
  const remaining = landscape ? formatRemaining(item.positionTicks, item.runtimeTicks) : null
  // A cached `primaryImageTag` can outlive the artwork on the server, and a
  // failing <img> re-requests on every re-render — which means a fresh round
  // trip to Jellyfin each time. Fall back to the title placeholder instead.
  const [imageIndex, setImageIndex] = useState(0)
  const images = landscape
    ? landscapeImageCandidates(item)
    : item.primaryImageTag
      ? [imageUrl(item)]
      : []
  const image = images[imageIndex]

  return (
    <article
      ref={observeTechnical}
      {...handlers}
      // While the expanded panel is over this card, its own lift would push the
      // artwork out from under an opaque panel that is not going to move with
      // it — a sliver of the poster creeping past the panel's edge.
      data-expanded={expanded || undefined}
      className={cn(
        "signal-card group flex flex-col gap-2",
        landscape ? "w-landscape-w" : "w-poster-w",
        className,
      )}
    >
      {/* `media-frame` carries the corner brackets on hover. They are drawn
          inside its own box, so clipping the artwork here does not touch
          them. */}
      <div
        className={cn(
          "media-frame relative overflow-hidden rounded-media bg-card ring-1 ring-white/5",
          landscape ? "h-landscape-h w-landscape-w" : "h-poster-h w-poster-w",
        )}
      >
        <Link
          to={`/item/${encodeURIComponent(item.id)}`}
          state={detailNavigationState(location)}
          aria-label={`Open details for ${item.name}`}
          className="absolute inset-0 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        >
          {image ? (
            <img
              src={image}
              alt=""
              // No `loading="lazy"`: the virtualizer already mounts only the rows
              // near the viewport, so lazy loading just adds a second deferral —
              // the overscan rows would wait for an intersection check instead of
              // fetching ahead, and the poster would land after the row is
              // already on screen. `decoding="async"` keeps the decode off the
              // main thread so a newly revealed row cannot drop a scroll frame.
              // No zoom on hover: the card is framed rather than enlarged, so a
              // shelf keeps its rhythm while the pointer runs along it.
              decoding="async"
              onError={() => setImageIndex((current) => current + 1)}
              className="media-artwork-image h-full w-full object-cover"
            />
          ) : (
            <span className="flex h-full w-full items-center justify-center px-3 text-center text-xs text-muted-foreground">
              {item.name}
            </span>
          )}
          <RatingOverlay item={item} hasRibbon={Boolean(ribbon)} />
          {ribbon && (
            <div className="data-label absolute top-0 right-0 z-[4] bg-primary px-1.5 py-1 leading-none text-primary-foreground">
              {ribbon}
            </div>
          )}
          <CardTechnicalReadout item={item} active={technicalVisible} />
          {/* Watched is drawn as a finished progress rule rather than a corner
              badge. The two states use the same thickness and accent fill;
              watched simply fills the track completely. */}
          {(progress > 0 || item.played) && (
            <div
              className="absolute inset-x-0 bottom-0 z-[7] h-[3px] bg-black/65"
              title={progress > 0 ? undefined : "Watched"}
            >
              <div className="h-full bg-primary" style={{ width: `${(progress || 1) * 100}%` }} />
            </div>
          )}
        </Link>
        {!previewsEnabled && <CardInlineActions item={item} />}
      </div>
      <Link
        to={`/item/${encodeURIComponent(item.id)}`}
        state={detailNavigationState(location)}
        className="min-w-0 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <div className="truncate text-sm font-medium transition-colors group-hover:text-primary">
          {item.name}
        </div>
        {(subtitle || remaining) && (
          <div className="data-value flex min-w-0 items-center gap-1.5 text-muted-foreground">
            {subtitle && <span className="truncate">{subtitle}</span>}
            {subtitle && remaining && (
              <span className="shrink-0 text-primary/50" aria-hidden>
                /
              </span>
            )}
            {remaining && <span className="shrink-0">{remaining}</span>}
          </div>
        )}
      </Link>
    </article>
  )
})
