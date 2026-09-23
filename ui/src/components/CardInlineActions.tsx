import { toast } from "sonner"
import { ItemActionButtons } from "@/components/ItemActionButtons"
import type { ItemSummary } from "@/lib/api"
import { useQualityOverride } from "@/lib/playback-quality"
import { useNextUp, usePlay, useSetFavorite, useSetPlayed } from "@/lib/queries"

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
  return (
    <div className="card-inline-actions">
      <ItemActionButtons
        variant="card"
        item={item}
        play={{ label: playLabel, disabled: play.isPending || resolvingPlayTarget, onPlay: () => void startPlayback() }}
        favorite={setFavorite}
        played={setPlayed}
        playedContext={playedContext}
      />
    </div>
  )
}
