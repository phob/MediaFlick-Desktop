import { Check, ExternalLink, Heart, Play, RotateCcw } from "lucide-react"
import { toast } from "sonner"
import { RequestSeasonsButton } from "@/components/seerr/RequestSeasonsButton"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  STREAMING_QUALITIES,
  externalLinksFor,
  progressFraction,
  qualityLabel,
  type ItemDetail,
  type StreamingQualityId,
} from "@/lib/api"
import { formatRemaining } from "@/lib/format"
import { setQualityOverride, useQualityOverride } from "@/lib/playback-quality"
import {
  useOpenExternal,
  usePlay,
  useSetFavorite,
  useSetPlayed,
  useSettings,
} from "@/lib/queries"

/** Stands in for "no override" — Radix Select cannot hold an empty value. */
const FROM_SETTINGS = "__settings__"

/**
 * A per-session override of the streaming quality saved in Settings.
 * It applies from the next playback on, so it lives next to the Play button
 * rather than in a settings dialog.
 */
function QualityPicker() {
  const override = useQualityOverride()
  const settings = useSettings()
  const saved = qualityLabel(settings.data?.streamingQuality)

  return (
    <Select
      value={override ?? FROM_SETTINGS}
      onValueChange={(value) => {
        const quality = value === FROM_SETTINGS ? null : (value as StreamingQualityId)
        setQualityOverride(quality)
        const label = quality ? qualityLabel(quality) : (saved ?? "the saved setting")
        toast.success(`Next playback uses ${label}.`)
      }}
    >
      <SelectTrigger size="sm" aria-label="Streaming quality">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={FROM_SETTINGS}>
          Quality: from settings{saved ? ` (${saved})` : ""}
        </SelectItem>
        {STREAMING_QUALITIES.map((quality) => (
          <SelectItem key={quality.id} value={quality.id}>
            Quality: {quality.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** Opens the item on IMDb, TMDb, or TVDB in the system browser. */
function ExternalLinks({ item }: { item: ItemDetail }) {
  const open = useOpenExternal()
  const links = externalLinksFor(item)
  if (!links.length) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="secondary" size="icon-lg" aria-label="Open on an external database">
          <ExternalLink className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {links.map((link) => (
          <DropdownMenuItem
            key={link.id}
            onSelect={() => open.mutate({ id: item.id, provider: link.id })}
          >
            <ExternalLink className="size-4" />
            View on {link.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * The action bar under the title. `playTarget` is what Play actually starts:
 * for a series that is its Next Up episode, so the primary button is never a
 * request the shell cannot serve.
 */
export function DetailActions({
  item,
  playTarget,
  playLabel,
}: {
  item: ItemDetail
  playTarget?: { id: string; positionTicks: number; runtimeTicks: number | null } | null
  playLabel?: string
}) {
  const play = usePlay()
  const setPlayed = useSetPlayed()
  const setFavorite = useSetFavorite()
  const quality = useQualityOverride() ?? undefined

  const isContainer = item.kind === "Series" || item.kind === "Season"
  const target = playTarget ?? (isContainer ? null : item)
  const resumable = Boolean(target && target.positionTicks > 0)
  const remaining = target ? formatRemaining(target.positionTicks, target.runtimeTicks) : null
  const progress = target ? progressFraction(target) : 0

  return (
    <div className="flex flex-col gap-3">
      {progress > 0 && (
        <div className="flex max-w-md items-center gap-3">
          <div className="h-[3px] flex-1 overflow-hidden bg-secondary">
            <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
          </div>
          {remaining && (
            <span className="data-value whitespace-nowrap text-muted-foreground">{remaining}</span>
          )}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {target && (
          <Button
            size="lg"
            className="min-w-28 shadow-lg shadow-primary/20 hover:bg-primary/85"
            disabled={play.isPending}
            onClick={() => play.mutate({ id: target.id, resume: resumable, quality })}
          >
            <Play className="size-4 fill-current" />
            {playLabel ?? (resumable ? "Resume" : "Play")}
          </Button>
        )}
        {target && resumable && (
          <Button
            variant="secondary"
            size="lg"
            className="hover:text-primary"
            disabled={play.isPending}
            onClick={() => play.mutate({ id: target.id, resume: false, quality })}
          >
            <RotateCcw className="size-4" />
            From start
          </Button>
        )}
        <Button
          variant="secondary"
          size="lg"
          className="hover:text-primary"
          onClick={() => setPlayed.mutate({ id: item.id, played: !item.played })}
        >
          <Check className="size-4" />
          {item.played ? "Watched" : "Mark watched"}
        </Button>
        <Button
          variant="secondary"
          size="icon-lg"
          className="hover:text-primary"
          aria-label={item.favorite ? "Remove from favorites" : "Add to favorites"}
          aria-pressed={item.favorite}
          onClick={() => setFavorite.mutate({ id: item.id, favorite: !item.favorite })}
        >
          {/* The accent, not the conventional red heart: destructive is the one
              colour in this palette that means something has gone wrong. */}
          <Heart className={item.favorite ? "size-4 fill-current text-primary" : "size-4"} />
        </Button>
        <RequestSeasonsButton item={item} />
        <ExternalLinks item={item} />
        {!isContainer && <QualityPicker />}
      </div>
    </div>
  )
}
