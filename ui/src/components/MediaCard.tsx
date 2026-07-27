import { memo, useState } from "react"
import { Link } from "react-router-dom"
import {
  imageUrl,
  landscapeImageCandidates,
  progressFraction,
  type ItemSummary,
} from "@/lib/api"
import { formatRemaining } from "@/lib/format"
import { cn } from "@/lib/utils"

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
}: {
  item: ItemSummary
  className?: string
  landscape?: boolean
}) {
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
    <Link
      to={`/item/${encodeURIComponent(item.id)}`}
      className={cn(
        "group flex flex-col gap-2 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring",
        landscape ? "w-landscape-w" : "w-poster-w",
        className,
      )}
    >
      <div
        className={cn(
          "relative overflow-hidden rounded-lg bg-card",
          landscape ? "h-landscape-h w-landscape-w" : "h-poster-h w-poster-w",
        )}
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
            decoding="async"
            onError={() => setImageIndex((current) => current + 1)}
            className="h-full w-full object-cover transition-transform duration-200 group-hover:scale-105"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center px-3 text-center text-xs text-muted-foreground">
            {item.name}
          </div>
        )}
        {item.played && (
          <div className="absolute right-2 top-2 rounded-full bg-primary px-2 py-0.5 text-[10px] font-medium text-primary-foreground">
            Watched
          </div>
        )}
        {progress > 0 && (
          <div className="absolute inset-x-0 bottom-0 h-1 bg-black/50">
            <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
          </div>
        )}
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm font-medium">{item.name}</div>
        {(subtitle || remaining) && (
          <div className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
            {subtitle && <span className="truncate">{subtitle}</span>}
            {subtitle && remaining && <span className="shrink-0" aria-hidden>·</span>}
            {remaining && <span className="shrink-0">{remaining}</span>}
          </div>
        )}
      </div>
    </Link>
  )
})
