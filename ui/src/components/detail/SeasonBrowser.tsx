import { Check, Play, Star } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"
import { CardTechnicalReadout } from "@/components/CardTechnicalReadout"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  THUMBNAIL_WIDTH,
  imageUrl,
  landscapeImageCandidates,
  progressFraction,
  type ItemSummary,
} from "@/lib/api"
import { formatCommunityRating, formatRuntime } from "@/lib/format"
import { useQualityOverride } from "@/lib/playback-quality"
import { usePreview } from "@/lib/preview"
import { usePlay, useSetPlayed } from "@/lib/queries"
import { cn } from "@/lib/utils"

/**
 * Rail order, not server order: Jellyfin numbers Specials as season 0 so it
 * arrives first, but it is the season people reach for last.
 */
export function seasonRailOrder(seasons: ItemSummary[]) {
  return [
    ...seasons.filter((season) => season.indexNumber !== 0),
    ...seasons.filter((season) => season.indexNumber === 0),
  ]
}

function SeasonPoster({ season, selected }: { season: ItemSummary; selected: boolean }) {
  const [failed, setFailed] = useState(false)
  return (
    <span
      className={cn(
        "relative block aspect-2/3 w-full overflow-hidden rounded-media bg-card",
        selected ? "ring-2 ring-primary" : "ring-1 ring-white/5",
      )}
    >
      {season.primaryImageTag && !failed ? (
        <img
          src={imageUrl(season)}
          alt=""
          decoding="async"
          loading="lazy"
          onError={() => setFailed(true)}
          className="media-artwork-image h-full w-full object-cover"
        />
      ) : (
        <span className="grid h-full place-items-center px-1 text-center text-xs text-muted-foreground">
          {season.name}
        </span>
      )}
    </span>
  )
}

/**
 * The season posters are the season switcher: the wall of covers a series page
 * used to link through collapses into one selectable rail above the episodes.
 */
function SeasonRail({
  seasons,
  selectedId,
  onSelect,
}: {
  seasons: ItemSummary[]
  selectedId: string | null
  onSelect: (season: ItemSummary) => void
}) {
  return (
    // The selection ring is a box-shadow outside the poster, and this list
    // clips its own overflow to scroll; the padding gives the ring room on the
    // top and left edges, and the negative margin keeps the posters aligned
    // with the grid below.
    <ul
      aria-label="Seasons"
      className="media-strip -mx-1 -mt-1 flex gap-4 overflow-x-auto px-1 pt-1 pb-2"
    >
      {seasons.map((season) => {
        const selected = season.id === selectedId
        return (
          <li key={season.id} className="shrink-0">
            <button
              type="button"
              aria-pressed={selected}
              onClick={() => onSelect(season)}
              className={cn(
                "flex w-36 flex-col gap-1.5 rounded-media outline-none transition-opacity focus-visible:ring-2 focus-visible:ring-ring",
                selected ? "opacity-100" : "opacity-60 hover:opacity-100 focus-visible:opacity-100",
              )}
            >
              <SeasonPoster season={season} selected={selected} />
              <span
                className={cn(
                  "data-value w-full truncate text-center",
                  selected ? "text-primary" : "text-muted-foreground",
                )}
              >
                {season.name}
              </span>
            </button>
          </li>
        )
      })}
    </ul>
  )
}

function EpisodeCard({
  episode,
  parentId,
  nextUp,
}: {
  episode: ItemSummary
  parentId: string
  nextUp: boolean
}) {
  const play = usePlay()
  const setPlayed = useSetPlayed()
  const quality = useQualityOverride() ?? undefined
  const { handlers, expanded } = usePreview(episode)
  // Same fallback ladder as every other landscape card: still, Thumb art,
  // backdrop — and a broken image steps down once instead of re-requesting.
  const [imageIndex, setImageIndex] = useState(0)
  const image = landscapeImageCandidates(episode, THUMBNAIL_WIDTH)[imageIndex]
  const resumable = episode.positionTicks > 0
  const progress = progressFraction(episode)
  const runtime = formatRuntime(episode.runtimeTicks)
  const rating = formatCommunityRating(episode.communityRating)

  return (
    <li
      {...handlers}
      data-expanded={expanded || undefined}
      data-next-up={nextUp || undefined}
      className="signal-card catalog-card group flex flex-col gap-2"
    >
      <div
        className={cn(
          "media-frame relative aspect-video overflow-hidden rounded-media bg-card",
          // The accent ring is the "you are here" mark: the next-up season is
          // preselected, and this pins the eye to the episode inside it.
          nextUp ? "ring-2 ring-primary" : "ring-1 ring-white/5",
        )}
      >
        <Link
          to={`/item/${encodeURIComponent(episode.id)}`}
          aria-label={episode.name}
          className="absolute inset-0 outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {image ? (
            <img
              src={image}
              alt=""
              decoding="async"
              loading="lazy"
              onError={() => setImageIndex((current) => current + 1)}
              className="media-artwork-image h-full w-full object-cover"
            />
          ) : (
            <span className="grid h-full place-items-center px-2 text-center text-xs text-muted-foreground">
              {episode.indexNumber != null ? `Episode ${episode.indexNumber}` : episode.name}
            </span>
          )}
        </Link>
        <CardTechnicalReadout item={episode} />
        {/* The controls stay mounted so keyboard users can reach them; only
            their opacity follows the pointer. */}
        <div className="absolute inset-x-0 bottom-0 z-[2] flex items-center gap-2 bg-gradient-to-t from-black/75 to-transparent p-2 pt-8 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <Button
            size="sm"
            disabled={play.isPending}
            onClick={() => play.mutate({ id: episode.id, resume: resumable, quality })}
          >
            <Play className="size-3 fill-current" />
            {resumable ? "Resume" : "Play"}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() =>
              setPlayed.mutate({ id: episode.id, played: !episode.played, context: parentId })
            }
          >
            <Check className="size-3" />
            {episode.played ? "Unwatch" : "Watched"}
          </Button>
        </div>
        {/* Watched is drawn as a finished progress rule rather than a corner
            badge, matching every other media card: the two states share the
            accent fill and watched simply fills the track completely. */}
        {(progress > 0 || episode.played) && (
          <div
            className="pointer-events-none absolute inset-x-0 bottom-0 z-[3] h-[3px] bg-black/65"
            title={progress > 0 ? undefined : "Watched"}
          >
            <div className="h-full bg-primary" style={{ width: `${(progress || 1) * 100}%` }} />
          </div>
        )}
      </div>
      <div className="min-w-0">
        <Link
          to={`/item/${encodeURIComponent(episode.id)}`}
          className={cn(
            "block truncate rounded-sm text-sm font-medium outline-none transition-colors group-hover:text-primary focus-visible:ring-2 focus-visible:ring-ring",
            episode.played && "text-muted-foreground",
          )}
        >
          {episode.indexNumber != null && (
            <span className="mr-2 text-muted-foreground tabular-nums">{episode.indexNumber}.</span>
          )}
          {episode.name}
          {nextUp && <span className="sr-only"> (Next up)</span>}
        </Link>
        {(rating || runtime) && (
          <div className="data-value flex items-center gap-3 text-muted-foreground">
            {rating && (
              <span
                className="flex items-center gap-1 text-foreground"
                title={`Jellyfin community rating: ${rating} out of 10`}
                aria-label={`Jellyfin community rating ${rating} out of 10`}
              >
                <Star className="size-3 fill-current text-amber-400" aria-hidden />
                {rating}
              </span>
            )}
            {runtime && <span>{runtime}</span>}
          </div>
        )}
      </div>
    </li>
  )
}

const GRID_CLASS = "grid grid-cols-[repeat(auto-fill,minmax(20rem,1fr))] gap-[var(--card-gap)]"

/**
 * A season's episodes as still-frame cards: an episode is recognized by its
 * frame, so the synopsis moves to the episode's own page and the card keeps
 * only what picking one needs — number, title, rating, runtime, progress.
 */
export function EpisodeGrid({
  episodes,
  parentId,
  nextUpEpisodeId = null,
}: {
  episodes: ItemSummary[]
  parentId: string
  nextUpEpisodeId?: string | null
}) {
  if (!episodes.length) {
    return <p className="text-sm text-muted-foreground">This season has no episodes.</p>
  }
  return (
    <ul className={GRID_CLASS}>
      {episodes.map((episode) => (
        <EpisodeCard
          key={episode.id}
          episode={episode}
          parentId={parentId}
          nextUp={episode.id === nextUpEpisodeId}
        />
      ))}
    </ul>
  )
}

function EpisodeGridSkeleton() {
  return (
    <div className={GRID_CLASS}>
      {Array.from({ length: 6 }, (_, index) => (
        <div key={index} className="flex flex-col gap-2">
          <Skeleton className="aspect-video w-full rounded-media" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      ))}
    </div>
  )
}

/** The series page's episode browser: the season rail over the episode grid. */
export function SeasonBrowser({
  seasons,
  selectedSeason,
  onSelect,
  episodes,
  episodesPending,
  episodesError,
  onRetry,
  nextUpEpisodeId,
}: {
  seasons: ItemSummary[]
  selectedSeason: ItemSummary | null
  onSelect: (season: ItemSummary) => void
  episodes: ItemSummary[]
  episodesPending: boolean
  episodesError: Error | null
  onRetry: () => void
  nextUpEpisodeId: string | null
}) {
  if (!seasons.length) return null
  return (
    <section className="flex flex-col gap-5 px-6 sm:px-10 lg:px-14">
      <h2 className="section-title">Episodes</h2>
      <SeasonRail
        seasons={seasons}
        selectedId={selectedSeason?.id ?? null}
        onSelect={onSelect}
      />
      {episodesPending ? (
        <EpisodeGridSkeleton />
      ) : episodesError ? (
        <div className="flex items-center gap-3 text-sm text-muted-foreground">
          <span>Could not load this season's episodes.</span>
          <Button variant="outline" size="sm" onClick={onRetry}>
            Try again
          </Button>
        </div>
      ) : (
        <EpisodeGrid
          episodes={episodes}
          parentId={selectedSeason?.id ?? ""}
          nextUpEpisodeId={nextUpEpisodeId}
        />
      )}
    </section>
  )
}
