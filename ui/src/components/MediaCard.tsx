import { concealEpisode, useSpoilerProtection } from "@/lib/viewing"
import { memo, useEffect, useId, useState } from "react"
import { Link } from "react-router-dom"
import { CardInlineActions } from "@/components/CardInlineActions"
import { CardTechnicalReadout } from "@/components/CardTechnicalReadout"
import { RatingOverlay } from "@/components/RatingOverlay"
import {
  imageUrl,
  landscapeImageCandidates,
  progressFraction,
  type ItemSummary,
} from "@/lib/api"
import { artworkShape } from "@/lib/artwork"
import { formatRemaining } from "@/lib/format"
import { usePreview } from "@/lib/preview"
import { isCatalogKind } from "@/lib/libraries"
import { useDetailNavigationState } from "@/hooks/use-detail-navigation-state"
import { cn } from "@/lib/utils"

const TECHNICAL_PREFETCH_MARGIN = "160px"

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

export const MediaCard = memo(function MediaCard({
  item: originalItem,
  className,
  landscape = false,
  ribbon,
  preview = true,
  interactive = true,
}: {
  item: ItemSummary
  className?: string
  landscape?: boolean
  ribbon?: string
  preview?: boolean
  interactive?: boolean
}) {
  const protectedEpisode = useSpoilerProtection(originalItem)
  const item = protectedEpisode ? concealEpisode(originalItem) : originalItem
  const navigationState = useDetailNavigationState()
  const catalogItem = isCatalogKind(item.kind)
  const { handlers, expanded, previewsEnabled } = usePreview(item, preview && catalogItem)
  const { observe: observeTechnical, visible: technicalVisible } = useTechnicalVisibility()
  const progress = progressFraction(item)
  const subtitle = subtitleFor(item)
  const remaining = landscape ? formatRemaining(item.positionTicks, item.runtimeTicks) : null
  const [imageIndex, setImageIndex] = useState(0)
  const images = protectedEpisode ? [] : landscape
    ? landscapeImageCandidates(item)
    : item.primaryImageTag
      ? [imageUrl(item)]
      : []
  const image = images[imageIndex]
  const unplayed = item.kind === "Series" ? item.unplayedCount ?? 0 : 0
  const unplayedId = useId()
  const detailPath = `/item/${encodeURIComponent(item.id)}`

  const artwork = (
    <>
      {image ? (
        <img
          src={image}
          alt=""
          decoding="async"
          onError={() => setImageIndex((current) => current + 1)}
          className={cn(
            "media-artwork-image h-full w-full",
            !landscape && artworkShape(item.primaryAspect) === "square" ? "object-contain" : "object-cover",
          )}
        />
      ) : (
        <span className="flex h-full w-full items-center justify-center px-3 text-center text-xs text-muted-foreground">
          {item.name}
        </span>
      )}
      {catalogItem && <RatingOverlay item={item} topRightOccupied={Boolean(ribbon) || unplayed > 0} />}
      {(ribbon || unplayed > 0) && (
        <div className="absolute top-0 right-0 z-[4] flex flex-col items-end gap-px">
          {ribbon && (
            <div className="data-label bg-primary px-1.5 py-1 leading-none text-primary-foreground">
              {ribbon}
            </div>
          )}
          {unplayed > 0 && (
            <div
              id={unplayedId}
              data-unplayed-count={unplayed}
              title={`${unplayed} unwatched ${unplayed === 1 ? "episode" : "episodes"}`}
              className="data-label min-w-6 bg-primary px-1.5 py-1 text-center leading-none text-primary-foreground"
            >
              <span aria-hidden="true">{unplayed.toLocaleString()}</span>
              <span className="sr-only">
                {unplayed} unwatched {unplayed === 1 ? "episode" : "episodes"}
              </span>
            </div>
          )}
        </div>
      )}
      <CardTechnicalReadout item={item} active={technicalVisible} />
      {(progress > 0 || item.played) && (
        <div
          className="absolute inset-x-0 bottom-0 z-[7] h-[3px] bg-scrim/65"
          title={progress > 0 ? undefined : "Watched"}
        >
          <div className="h-full bg-primary" style={{ width: `${(progress || 1) * 100}%` }} />
        </div>
      )}
    </>
  )

  const caption = (
    <>
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
    </>
  )

  return (
    <article
      ref={observeTechnical}
      {...handlers}
      data-expanded={expanded || undefined}
      className={cn(
        "signal-card group flex flex-col gap-2",
        landscape ? "w-landscape-w" : "w-poster-w",
        className,
      )}
    >
      <div
        className={cn(
          "media-frame relative overflow-hidden rounded-media bg-card ring-1 ring-hairline",
          landscape ? "h-landscape-h w-landscape-w" : "h-poster-h w-poster-w",
        )}
      >
        {interactive ? (
          <Link
            to={detailPath}
            state={navigationState}
            aria-label={`Open details for ${item.name}`}
            aria-describedby={unplayed > 0 ? unplayedId : undefined}
            className="absolute inset-0 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
          >
            {artwork}
          </Link>
        ) : (
          <div className="absolute inset-0">{artwork}</div>
        )}
        {catalogItem && !previewsEnabled && <CardInlineActions item={item} interactive={interactive} />}
      </div>
      {interactive ? (
        <Link
          to={detailPath}
          state={navigationState}
          className="min-w-0 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {caption}
        </Link>
      ) : (
        <div className="min-w-0 rounded-sm">{caption}</div>
      )}
    </article>
  )
})
