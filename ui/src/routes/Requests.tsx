import { Inbox, X } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"
import { PageEmptyState, PageHeader } from "@/components/PageHeader"
import { SeerrRequestStatusBadge, SeerrStatusBadge } from "@/components/seerr/SeerrStatusBadge"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { seerrImageUrl, type SeerrRequest } from "@/lib/api"
import { useSeerrCancelRequest, useSeerrMedia, useSeerrRequests } from "@/lib/queries"

const FILTERS = [
  { id: "all", label: "All" },
  { id: "pending", label: "Awaiting approval" },
  { id: "processing", label: "Downloading" },
  { id: "available", label: "In your library" },
] as const

function requestedSeasons(request: SeerrRequest) {
  if (request.mediaType !== "tv" || !request.seasons.length) return null
  const numbers = [...request.seasons].sort((a, b) => a - b)
  return numbers.length === 1
    ? `Season ${numbers[0]}`
    : `Seasons ${numbers.slice(0, -1).join(", ")} and ${numbers[numbers.length - 1]}`
}

function requestedOn(request: SeerrRequest) {
  if (!request.createdAt) return null
  const date = new Date(request.createdAt)
  return Number.isNaN(date.valueOf()) ? null : date.toLocaleDateString()
}

/**
 * One request. The title is not on the request itself — Seerr's rows reference
 * a TMDB id and nothing more — so it is resolved through the media detail query,
 * which is cached per title and shared with every other surface that shows it.
 */
function RequestCard({ request }: { request: SeerrRequest }) {
  const media = useSeerrMedia(request.mediaType, request.tmdbId)
  const cancel = useSeerrCancelRequest()
  const poster = seerrImageUrl(media.data?.posterPath, "w154")
  const backdrop = seerrImageUrl(media.data?.backdropPath, "w780")
  const seasons = requestedSeasons(request)
  const date = requestedOn(request)

  return (
    <li className="group relative flex min-h-32 items-center gap-4 overflow-hidden rounded-xl border border-white/5 bg-card/55 p-4 shadow-lg shadow-black/10 transition hover:border-white/10 hover:bg-card/75">
      {backdrop && (
        <div className="pointer-events-none absolute inset-y-0 right-0 w-2/3 opacity-15 transition-opacity group-hover:opacity-20">
          <img src={backdrop} alt="" decoding="async" className="media-backdrop-image h-full w-full object-cover" />
          <div className="absolute inset-0 bg-linear-to-r from-card via-card/50 to-transparent" />
          <div className="absolute inset-0 bg-linear-to-t from-card/70 to-transparent" />
        </div>
      )}
      <div className="relative z-10 h-28 w-[4.7rem] shrink-0 overflow-hidden rounded-lg bg-card shadow-xl ring-1 ring-white/10">
        {poster && <img src={poster} alt="" decoding="async" className="media-artwork-image h-full w-full object-cover" />}
      </div>

      <div className="relative z-10 flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="truncate text-base font-semibold">
            {media.data?.title ?? (media.isPending ? "…" : `TMDB ${request.tmdbId}`)}
          </span>
          {media.data?.year && (
            <span className="text-sm text-muted-foreground">{media.data.year}</span>
          )}
          {request.is4k && <Badge variant="outline">4K</Badge>}
        </div>
        <div className="truncate text-xs text-muted-foreground">
          {[request.mediaType === "tv" ? "Series" : "Movie", seasons, date && `requested ${date}`]
            .filter(Boolean)
            .join(" · ")}
        </div>
        <div className="flex flex-wrap items-center gap-2 pt-1">
          <SeerrRequestStatusBadge
            status={request.status}
            suppressUnknown={Boolean(request.libraryItemId) || request.mediaStatus !== "unknown"}
          />
          {request.libraryItemId ? <Badge>In your library</Badge> : <SeerrStatusBadge status={request.mediaStatus} />}
        </div>
      </div>

      <div className="relative z-10 flex shrink-0 items-center gap-2">
        {request.libraryItemId && (
          <Button asChild variant="secondary" size="sm">
            <Link to={`/item/${encodeURIComponent(request.libraryItemId)}`}>Open in library</Link>
          </Button>
        )}
        {/* Offered only while it can still be withdrawn. A refusal from Seerr
            surfaces as a permission error and leaves the session alone. */}
        {request.status === "pending" && (
          <Button
            variant="ghost"
            size="sm"
            disabled={cancel.isPending}
            onClick={() => cancel.mutate(request.id)}
          >
            <X className="size-4" />
            Cancel
          </Button>
        )}
      </div>
    </li>
  )
}

/** Your own requests — never the household's; Seerr shows those to admins. */
export default function Requests() {
  const [filter, setFilter] = useState<string>("all")
  const requests = useSeerrRequests(filter)

  return (
    <div className="flex min-h-full min-w-0 flex-col">
      <PageHeader
        eyebrow="Seerr"
        title="Your requests"
        description="Track approvals, downloads, and the titles that have landed in your library."
        contentClassName="max-w-6xl"
        actions={
          <Tabs value={filter} onValueChange={setFilter}>
            <TabsList className="h-11 rounded-xl border border-white/5 bg-white/5 p-1">
              {FILTERS.map((entry) => (
                <TabsTrigger
                  key={entry.id}
                  value={entry.id}
                  className="h-full rounded-media px-4 data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  {entry.label}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        }
      />

      <div className="px-6 pb-10 sm:px-10 lg:px-14">
        <div className="max-w-6xl">
          {requests.error ? (
            <p className="text-sm text-destructive">{requests.error.message}</p>
          ) : requests.isPending ? (
            <ul className="flex flex-col gap-3">
              {Array.from({ length: 4 }, (_, index) => (
                <Skeleton key={index} className="h-36 rounded-xl" />
              ))}
            </ul>
          ) : requests.data?.results.length ? (
            <ul className="flex flex-col gap-3">
              {requests.data.results.map((request) => (
                <RequestCard key={request.id} request={request} />
              ))}
            </ul>
          ) : (
            <PageEmptyState
              icon={<Inbox className="size-6" />}
              title="No requests here yet"
              description="Discover something outside your library and request it. Its approval and download progress will appear here."
            />
          )}
        </div>
      </div>
    </div>
  )
}
