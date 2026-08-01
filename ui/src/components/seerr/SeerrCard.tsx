import { ArrowUpRight, Star } from "lucide-react"
import { memo, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { SeerrStatusBadge } from "@/components/seerr/SeerrStatusBadge"
import { Badge } from "@/components/ui/badge"
import { seerrImageUrl, type SeerrResult } from "@/lib/api"
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
  className,
}: {
  result: SeerrResult
  className?: string
}) {
  const location = useLocation()
  const subtitle = [result.year, result.mediaType === "tv" ? "Series" : "Film"]
    .filter(Boolean)
    .join(" · ")
  const rating =
    result.voteAverage && result.voteAverage > 0
      ? result.voteAverage.toFixed(1)
      : null
  const shell = cn(
    "catalog-card group flex w-poster-w flex-col gap-2 rounded-lg text-left outline-none focus-visible:ring-2 focus-visible:ring-ring",
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

  return (
    <Link
      to={{
        pathname: `/discover/${result.mediaType}/${result.tmdbId}`,
        // Keeping the browse query on the detail URL makes the explicit Back
        // link as reliable as browser Back, including after a reload.
        search: location.pathname === "/discover" ? location.search : "",
      }}
      className={shell}
    >
      <Poster result={result} />
      {caption}
    </Link>
  )
})
