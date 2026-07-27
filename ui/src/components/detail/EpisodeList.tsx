import { Check, Play } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { THUMBNAIL_WIDTH, imageUrl, progressFraction, type ItemSummary } from "@/lib/api"
import { formatRuntime } from "@/lib/format"
import { useQualityOverride } from "@/lib/playback-quality"
import { usePlay, useSetPlayed } from "@/lib/queries"
import { cn } from "@/lib/utils"

function Thumbnail({ episode }: { episode: ItemSummary }) {
  const [failed, setFailed] = useState(false)
  const progress = progressFraction(episode)

  return (
    <div className="relative aspect-video w-44 shrink-0 overflow-hidden rounded-md bg-card">
      {episode.primaryImageTag && !failed ? (
        <img
          src={imageUrl(episode, "Primary", THUMBNAIL_WIDTH)}
          alt=""
          decoding="async"
          onError={() => setFailed(true)}
          className="h-full w-full object-cover"
        />
      ) : (
        <div className="grid h-full place-items-center px-2 text-center text-xs text-muted-foreground">
          {episode.indexNumber != null ? `Episode ${episode.indexNumber}` : episode.name}
        </div>
      )}
      {episode.played && (
        <div className="absolute right-1.5 top-1.5 grid size-5 place-items-center rounded-full bg-primary text-primary-foreground">
          <Check className="size-3" aria-hidden />
        </div>
      )}
      {progress > 0 && (
        <div className="absolute inset-x-0 bottom-0 h-1 bg-black/60">
          <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
        </div>
      )}
    </div>
  )
}

function EpisodeRow({ episode, parentId }: { episode: ItemSummary; parentId: string }) {
  const play = usePlay()
  const setPlayed = useSetPlayed()
  const quality = useQualityOverride() ?? undefined
  const resumable = episode.positionTicks > 0
  const runtime = formatRuntime(episode.runtimeTicks)

  return (
    <li className="group flex gap-4 rounded-xl border border-transparent p-3 transition hover:border-white/5 hover:bg-card/75">
      <Link
        to={`/item/${encodeURIComponent(episode.id)}`}
        className="shrink-0 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Thumbnail episode={episode} />
      </Link>

      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-baseline gap-3">
          <Link
            to={`/item/${encodeURIComponent(episode.id)}`}
            className={cn(
              "truncate rounded-sm text-sm font-medium hover:underline",
              episode.played && "text-muted-foreground",
            )}
          >
            {episode.indexNumber != null && (
              <span className="mr-2 text-muted-foreground tabular-nums">{episode.indexNumber}.</span>
            )}
            {episode.name}
          </Link>
          {runtime && (
            <span className="ml-auto shrink-0 text-xs text-muted-foreground">{runtime}</span>
          )}
        </div>
        {episode.overview && (
          <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
            {episode.overview}
          </p>
        )}
        {/* The controls stay mounted so keyboard users can reach them; only
            their opacity follows the pointer. */}
        <div className="mt-auto flex items-center gap-2 pt-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
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
      </div>
    </li>
  )
}

/**
 * The episode list of a season: a reading surface rather than a poster wall,
 * because what an episode needs is its number, title, and synopsis, and those
 * do not fit under a 2:3 card.
 */
export function EpisodeList({ episodes, parentId }: { episodes: ItemSummary[]; parentId: string }) {
  if (!episodes.length) return null
  return (
    <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
      <h2 className="section-title">Episodes</h2>
      {/* Capped: a synopsis set in a 2800px line is unreadable, and the app
          window is as wide as the desktop. */}
      <ul className="flex max-w-6xl flex-col gap-1">
        {episodes.map((episode) => (
          <EpisodeRow key={episode.id} episode={episode} parentId={parentId} />
        ))}
      </ul>
    </section>
  )
}
