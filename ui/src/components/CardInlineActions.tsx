import { Check, Play, Plus, ThumbsUp } from "lucide-react"
import { toast } from "sonner"
import type { ItemSummary } from "@/lib/api"
import { useQualityOverride } from "@/lib/playback-quality"
import { useNextUp, usePlay, useSetFavorite, useSetPlayed } from "@/lib/queries"
import { cn } from "@/lib/utils"

/**
 * The expanded preview's three actions, kept on the artwork when that preview
 * is disabled. A series resolves its episode only after Play is pressed, so a
 * library wall does not issue one Next Up request per card.
 */
export function CardInlineActions({
  item,
  playedContext = item.seriesId,
}: {
  item: ItemSummary
  playedContext?: string | null
}) {
  const nextUp = useNextUp(item.kind === "Series" ? item.id : undefined, false)
  const play = usePlay()
  const setFavorite = useSetFavorite()
  const setPlayed = useSetPlayed()
  const quality = useQualityOverride() ?? undefined
  const resolvingPlayTarget = item.kind === "Series" && nextUp.isFetching

  const startPlayback = async () => {
    let target: ItemSummary | null = item
    if (item.kind === "Series") {
      const result = await nextUp.refetch()
      if (result.isError) {
        toast.error(result.error.message)
        return
      }
      target = result.data?.item ?? null
    }
    if (!target || (target.kind !== "Movie" && target.kind !== "Episode")) {
      toast.error("No playable episode was found.")
      return
    }
    play.mutate({ id: target.id, resume: target.positionTicks > 0, quality })
  }

  const playLabel = item.kind === "Series"
    ? "Play next episode"
    : item.positionTicks > 0
      ? "Resume"
      : "Play"
  const favoriteLabel = item.favorite ? "Remove from My List" : "Add to My List"
  const playedLabel = item.played ? "Mark as unwatched" : "Mark as watched"

  return (
    <div className="card-inline-actions">
      <button
        type="button"
        disabled={play.isPending || resolvingPlayTarget}
        aria-label={playLabel}
        title={playLabel}
        onClick={() => void startPlayback()}
        className="preview-action bg-primary text-primary-foreground hover:bg-primary/85"
      >
        <Play className="size-4 fill-current" />
      </button>
      <button
        type="button"
        disabled={setFavorite.isPending}
        aria-label={favoriteLabel}
        aria-pressed={item.favorite}
        title={favoriteLabel}
        onClick={() => setFavorite.mutate({ id: item.id, favorite: !item.favorite })}
        className={cn(
          "preview-action border bg-black/85",
          item.favorite
            ? "border-primary/70 text-primary"
            : "border-white/35 text-white hover:border-primary/70 hover:text-primary",
        )}
      >
        {item.favorite ? <Check className="size-4" /> : <Plus className="size-4" />}
      </button>
      <button
        type="button"
        disabled={setPlayed.isPending}
        aria-label={playedLabel}
        aria-pressed={item.played}
        title={playedLabel}
        onClick={() =>
          setPlayed.mutate({ id: item.id, played: !item.played, context: playedContext })
        }
        className={cn(
          "preview-action border bg-black/85",
          item.played
            ? "border-primary/70 text-primary"
            : "border-white/35 text-white hover:border-primary/70 hover:text-primary",
        )}
      >
        <ThumbsUp className={cn("size-4", item.played && "fill-current")} />
      </button>
    </div>
  )
}
