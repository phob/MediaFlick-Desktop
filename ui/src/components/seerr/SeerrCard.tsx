import { ArrowUpRight, Plus, Star } from "lucide-react"
import { memo, type MouseEvent, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { RequestDialog } from "@/components/seerr/RequestDialog"
import { SeerrStatusBadge } from "@/components/seerr/SeerrStatusBadge"
import { Badge } from "@/components/ui/badge"
import {
  seerrImageUrl,
  type SeerrCapabilities,
  type SeerrResult,
} from "@/lib/api"
import { canQuickRequest } from "@/lib/seerr-request"
import { cn } from "@/lib/utils"

function Poster({ result }: { result: SeerrResult }) {
  const [failed, setFailed] = useState(false)
  const src = seerrImageUrl(result.posterPath)

  return (
    <div className="relative h-poster-h w-poster-w overflow-hidden rounded-lg bg-card">
      {src && !failed ? (
        <img
          src={src}
          alt=""
          decoding="async"
          onError={() => setFailed(true)}
          className="media-artwork-image h-full w-full object-cover transition-transform duration-200 group-hover:scale-105"
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center px-3 text-center text-xs text-muted-foreground">
          {result.title}
        </div>
      )}
      <div className="absolute top-2 right-2 flex flex-col items-end gap-1">
        {result.libraryItemId ? (
          <Badge>In your library</Badge>
        ) : (
          <SeerrStatusBadge status={result.status} />
        )}
      </div>
    </div>
  )
}

/**
 * A title from Seerr, in the same geometry as [`MediaCard`] so a row of them
 * sits level with the library's own posters.
 *
 * Every card opens Seerr's full title record first. A title already in the
 * library can continue into the local detail page from there; a missing title
 * offers the request flow alongside its synopsis, release information, cast,
 * trailer, and availability.
 */
export const SeerrCard = memo(function SeerrCard({
  result,
  capabilities,
  className,
}: {
  result: SeerrResult
  capabilities?: SeerrCapabilities | null
  className?: string
}) {
  const location = useLocation()
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [requesting, setRequesting] = useState(false)
  const quickRequestable = canQuickRequest(result, capabilities)
  const quickRequestVisible = hovered || focused
  const subtitle = [result.year, result.mediaType === "tv" ? "Series" : "Film"]
    .filter(Boolean)
    .join(" · ")
  const rating =
    result.voteAverage && result.voteAverage > 0
      ? result.voteAverage.toFixed(1)
      : null
  const shell = cn(
    "catalog-card quick-request-card group relative flex w-poster-w flex-col rounded-lg text-left",
    className,
  )
  const caption = (
    <div className="min-w-0">
      <div className="truncate text-sm font-medium">{result.title}</div>
      <div className="data-value flex items-center gap-1.5 truncate text-muted-foreground">
        {rating ? (
          <span
            className="inline-flex items-center gap-1 text-primary"
            title={`TMDB rating: ${rating}/10`}
          >
            <Star className="size-3.5 fill-current" aria-hidden />
            {rating}
          </span>
        ) : null}
        {rating && subtitle ? <span className="text-border">/</span> : null}
        <span>{subtitle}</span>
      </div>
      <div className="data-label mt-1 flex h-4 items-center gap-1 text-foreground/65">
        <ArrowUpRight className="size-3" /> View details
      </div>
    </div>
  )

  const destination = {
    pathname: `/discover/${result.mediaType}/${result.tmdbId}`,
    // Keeping the browse query on the detail URL makes the explicit Back link
    // as reliable as browser Back, including after a reload.
    search: location.pathname === "/discover" ? location.search : "",
  }

  const openQuickRequest = (event: MouseEvent<HTMLButtonElement>) => {
    // The action overlays a card-sized detail link. Keep this guard even though
    // the button is its semantic sibling: it prevents a future wrapper change,
    // or a synthetic parent handler, from turning Request into navigation.
    event.preventDefault()
    event.stopPropagation()
    setRequesting(true)
  }

  return (
    <>
      <div
        className={shell}
        data-quick-request-card
        data-quick-request-visible={quickRequestVisible || undefined}
        onPointerEnter={() => setHovered(true)}
        onPointerLeave={() => setHovered(false)}
        onFocusCapture={() => setFocused(true)}
        onBlurCapture={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
            setFocused(false)
          }
        }}
      >
        <Link
          to={destination}
          className="flex flex-col gap-2 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Poster result={result} />
          {caption}
        </Link>
        {quickRequestable && (
          <button
            type="button"
            aria-label={`Request ${result.title}`}
            aria-haspopup="dialog"
            title={`Request ${result.title}`}
            onClick={openQuickRequest}
            className={cn(
              "quick-request-action absolute right-2 z-20 grid size-11 place-items-center rounded-media border border-primary/45 bg-background/90 text-primary shadow-lg shadow-black/60 backdrop-blur-sm outline-none transition-[opacity,transform,background-color,border-color] duration-150 hover:border-primary hover:bg-primary hover:text-primary-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
              quickRequestVisible
                ? "pointer-events-auto translate-y-0 opacity-100"
                : "pointer-events-none translate-y-1 opacity-0",
            )}
          >
            <Plus className="size-5" aria-hidden />
          </button>
        )}
      </div>
      {requesting && (
        <RequestDialog result={result} onClose={() => setRequesting(false)} />
      )}
    </>
  )
})
