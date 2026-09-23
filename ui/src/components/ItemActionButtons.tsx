import { Check, Play, Plus, ThumbsUp } from "lucide-react"
import type { MouseEvent } from "react"
import type { ItemSummary } from "@/lib/api"
import { cn } from "@/lib/utils"

export interface ItemActionMutation<Input> {
  isPending: boolean
  mutate: (input: Input) => void
}

export interface ItemPlayAction {
  label: string
  disabled: boolean
  onPlay: () => void
}

/**
 * `card` sits over artwork, so it carries its own dark backing; `panel` sits on
 * the expanded preview's card surface above its stretched details link.
 */
type ItemActionVariant = "card" | "panel"

const VARIANTS = {
  card: {
    icon: "size-4",
    button: "",
    toggle: "bg-black/85",
    off: "border-white/35 text-white hover:border-primary/70 hover:text-primary",
    favoriteOn: "border-primary/70 text-primary",
    playedOn: "border-primary/70 text-primary",
  },
  panel: {
    icon: "size-5",
    button: "relative z-20",
    toggle: "",
    off: "border-foreground/30 text-foreground/80 hover:border-primary/60 hover:text-primary",
    favoriteOn: "border-primary/70 text-primary",
    playedOn: "border-primary/70 bg-primary/15 text-primary",
  },
} satisfies Record<ItemActionVariant, Record<string, string>>

/**
 * Play, My List, and watched for one title. Jellyfin has no separate "liked";
 * watched is the nearest real state, and the thumb is how the row is read at a
 * glance. The toggles report their state through `aria-pressed`.
 */
export function ItemActionButtons({
  item,
  play,
  favorite,
  played,
  playedContext,
  variant,
}: {
  item: Pick<ItemSummary, "id" | "favorite" | "played">
  /** Omitted while there is nothing to play, such as a series without Next Up. */
  play?: ItemPlayAction | null
  favorite: ItemActionMutation<{ id: string; favorite: boolean }>
  played: ItemActionMutation<{ id: string; played: boolean; context?: string | null }>
  playedContext?: string | null
  variant: ItemActionVariant
}) {
  const styles = VARIANTS[variant]
  // The panel's background is itself a details link; its controls must not
  // also reach that handler.
  const act = (action: () => void) => (event: MouseEvent) => {
    if (variant === "panel") event.stopPropagation()
    action()
  }
  const favoriteLabel = item.favorite ? "Remove from My List" : "Add to My List"
  const playedLabel = item.played ? "Mark as unwatched" : "Mark as watched"
  return (
    <>
      {play && (
        <button
          type="button"
          disabled={play.disabled}
          aria-label={play.label}
          title={play.label}
          onClick={act(play.onPlay)}
          className={cn("preview-action bg-primary text-primary-foreground hover:bg-primary/85", styles.button)}
        >
          <Play className={cn(styles.icon, "fill-current")} />
        </button>
      )}
      <button
        type="button"
        disabled={favorite.isPending}
        aria-label={favoriteLabel}
        aria-pressed={item.favorite}
        title={favoriteLabel}
        onClick={act(() => favorite.mutate({ id: item.id, favorite: !item.favorite }))}
        className={cn(
          "preview-action border",
          styles.button,
          styles.toggle,
          item.favorite ? styles.favoriteOn : styles.off,
        )}
      >
        {item.favorite ? <Check className={styles.icon} /> : <Plus className={styles.icon} />}
      </button>
      <button
        type="button"
        disabled={played.isPending}
        aria-label={playedLabel}
        aria-pressed={item.played}
        title={playedLabel}
        onClick={act(() => played.mutate({ id: item.id, played: !item.played, context: playedContext }))}
        className={cn(
          "preview-action border",
          styles.button,
          styles.toggle,
          item.played ? styles.playedOn : styles.off,
        )}
      >
        <ThumbsUp className={cn(styles.icon, item.played && "fill-current")} />
      </button>
    </>
  )
}
