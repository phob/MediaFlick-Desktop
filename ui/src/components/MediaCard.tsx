import { memo, useState } from "react"
import { Link } from "react-router-dom"
import { imageUrl, progressFraction, type ItemSummary } from "@/lib/api"
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
}: {
  item: ItemSummary
  className?: string
}) {
  const progress = progressFraction(item)
  const subtitle = subtitleFor(item)
  // A cached `primaryImageTag` can outlive the artwork on the server, and a
  // failing <img> re-requests on every re-render — which means a fresh round
  // trip to Jellyfin each time. Fall back to the title placeholder instead.
  const [imageFailed, setImageFailed] = useState(false)
  const showImage = Boolean(item.primaryImageTag) && !imageFailed

  return (
    <Link
      to={`/item/${encodeURIComponent(item.id)}`}
      className={cn(
        "group flex w-poster-w flex-col gap-2 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring",
        className,
      )}
    >
      <div className="relative h-poster-h w-poster-w overflow-hidden rounded-lg bg-card">
        {showImage ? (
          <img
            src={imageUrl(item)}
            alt=""
            // No `loading="lazy"`: the virtualizer already mounts only the rows
            // near the viewport, so lazy loading just adds a second deferral —
            // the overscan rows would wait for an intersection check instead of
            // fetching ahead, and the poster would land after the row is
            // already on screen. `decoding="async"` keeps the decode off the
            // main thread so a newly revealed row cannot drop a scroll frame.
            decoding="async"
            onError={() => setImageFailed(true)}
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
        <div className="truncate text-sm">{item.name}</div>
        {subtitle && <div className="truncate text-xs text-muted-foreground">{subtitle}</div>}
      </div>
    </Link>
  )
})
