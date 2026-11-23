import { Check, Heart, Play } from "lucide-react"
import type { MouseEvent } from "react"
import type { ItemSummary } from "@/lib/api"
import { cn } from "@/lib/utils"
import { Pressable } from "@/components/ui/pressable"

export interface ItemActionMutation<Input> {
  isPending: boolean
  mutate: (input: Input) => void
}

interface ItemPlayAction {
  label: string
  disabled: boolean
  onPlay: () => void
}

type ItemActionVariant = "card" | "panel"

const VARIANTS = {
  card: {
    icon: "size-4",
    button: "",
    toggle: "bg-scrim/85",
    off: "border-white/35 text-white",
    offHover: "hover:border-primary/70 hover:text-primary",
    favoriteOn: "border-primary/70 text-primary",
    playedOn: "border-primary/70 text-primary",
  },
  panel: {
    icon: "size-5",
    button: "relative z-20",
    toggle: "",
    off: "border-foreground/30 text-foreground/80",
    offHover: "hover:border-primary/60 hover:text-primary",
    favoriteOn: "border-primary/70 text-primary",
    playedOn: "border-primary/70 bg-primary/15 text-primary",
  },
} satisfies Record<ItemActionVariant, Record<string, string>>

type ItemActionButtonsProps =
  | {
      item: Pick<ItemSummary, "id" | "favorite" | "played">
      play?: ItemPlayAction | null
      favorite: ItemActionMutation<{ id: string; favorite: boolean }>
      played: ItemActionMutation<{ id: string; played: boolean; context?: string | null }>
      playedContext?: string | null
      variant: ItemActionVariant
      interactive?: true
    }
  | {
      item: Pick<ItemSummary, "id" | "favorite" | "played">
      play?: boolean
      variant: ItemActionVariant
      interactive: false
    }

export function ItemActionButtons(props: ItemActionButtonsProps) {
  const { item, variant } = props
  const interactive = props.interactive !== false
  const styles = VARIANTS[variant]
  const playClass = cn(
    "preview-action bg-primary text-primary-foreground",
    interactive && "hover:bg-primary/85",
    styles.button,
  )
  const offClass = cn(styles.off, interactive && styles.offHover)
  const favoriteClass = cn(
    "preview-action border",
    styles.button,
    styles.toggle,
    item.favorite ? styles.favoriteOn : offClass,
  )
  const playedClass = cn(
    "preview-action border",
    styles.button,
    styles.toggle,
    item.played ? styles.playedOn : offClass,
  )

  if (props.interactive === false) {
    return (
      <>
        {props.play && (
          <span aria-hidden className={playClass}>
            <Play className={cn(styles.icon, "fill-current")} />
          </span>
        )}
        <span aria-hidden className={favoriteClass}>
          <Heart className={cn(styles.icon, item.favorite && "fill-current")} />
        </span>
        <span aria-hidden className={playedClass}>
          <Check className={styles.icon} />
        </span>
      </>
    )
  }

  const { play, favorite, played, playedContext } = props
  const act = (action: () => void) => (event: MouseEvent) => {
    if (variant === "panel") event.stopPropagation()
    action()
  }
  const favoriteLabel = item.favorite ? "Remove from favorites" : "Add to favorites"
  const playedLabel = item.played ? "Mark unwatched" : "Mark watched"
  return (
    <>
      {play && (
        <Pressable
          disabled={play.disabled}
          aria-label={play.label}
          title={play.label}
          onClick={act(play.onPlay)}
          className={playClass}
        >
          <Play className={cn(styles.icon, "fill-current")} />
        </Pressable>
      )}
      <Pressable
        disabled={favorite.isPending}
        aria-label={favoriteLabel}
        aria-pressed={item.favorite}
        title={favoriteLabel}
        onClick={act(() => favorite.mutate({ id: item.id, favorite: !item.favorite }))}
        className={favoriteClass}
      >
        <Heart className={cn(styles.icon, item.favorite && "fill-current")} />
      </Pressable>
      <Pressable
        disabled={played.isPending}
        aria-label={playedLabel}
        aria-pressed={item.played}
        title={playedLabel}
        onClick={act(() => played.mutate({ id: item.id, played: !item.played, context: playedContext }))}
        className={playedClass}
      >
        <Check className={styles.icon} />
      </Pressable>
    </>
  )
}
