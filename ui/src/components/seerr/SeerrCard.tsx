import { Plus } from "lucide-react"
import { memo, useState } from "react"
import { Link } from "react-router-dom"
import { RequestDialog } from "@/components/seerr/RequestDialog"
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
          className="h-full w-full object-cover transition-transform duration-200 group-hover:scale-105"
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
 * The card is one of two things depending on the join the shell already did: a
 * link to the cached item when the library has it, and the request dialog when
 * it does not. There is no third, dead-end state — which is the point of keying
 * the join on (kind, TMDB id) in the first place.
 */
export const SeerrCard = memo(function SeerrCard({
  result,
  className,
}: {
  result: SeerrResult
  className?: string
}) {
  const [requesting, setRequesting] = useState(false)
  const subtitle = [result.year, result.mediaType === "tv" ? "Series" : "Film"]
    .filter(Boolean)
    .join(" · ")
  const shell = cn(
    "catalog-card group flex w-poster-w flex-col gap-2 rounded-lg text-left outline-none focus-visible:ring-2 focus-visible:ring-ring",
    className,
  )
  const caption = (
    <div className="min-w-0">
      <div className="truncate text-sm font-medium">{result.title}</div>
      <div className="flex items-center gap-1 truncate text-xs text-muted-foreground">
        {result.libraryItemId ? subtitle : <><Plus className="size-3" /> Request · {subtitle}</>}
      </div>
    </div>
  )

  if (result.libraryItemId) {
    return (
      <Link to={`/item/${encodeURIComponent(result.libraryItemId)}`} className={shell}>
        <Poster result={result} />
        {caption}
      </Link>
    )
  }

  return (
    <>
      <button type="button" className={shell} onClick={() => setRequesting(true)}>
        <Poster result={result} />
        {caption}
      </button>
      {requesting && <RequestDialog result={result} onClose={() => setRequesting(false)} />}
    </>
  )
})
