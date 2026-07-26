import { Heart, Play, Check } from "lucide-react"
import { useParams } from "react-router-dom"
import { toast } from "sonner"
import { MediaCard } from "@/components/MediaCard"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import {
  STREAMING_QUALITIES,
  imageUrl,
  qualityLabel,
  ticksToMs,
  type StreamingQualityId,
} from "@/lib/api"
import { setQualityOverride, useQualityOverride } from "@/lib/playback-quality"
import {
  useChildren,
  useItem,
  usePlay,
  useSetFavorite,
  useSetPlayed,
  useSettings,
} from "@/lib/queries"

function formatRuntime(ticks: number | null) {
  const minutes = Math.round(ticksToMs(ticks) / 60_000)
  if (!minutes) return null
  const hours = Math.floor(minutes / 60)
  return hours ? `${hours}h ${minutes % 60}m` : `${minutes}m`
}

/** Stands in for "no override" — Radix Select cannot hold an empty value. */
const FROM_SETTINGS = "__settings__"

/**
 * A per-session override of the streaming quality saved in Client Settings.
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

export default function ItemDetail() {
  const { id } = useParams<{ id: string }>()
  const { data: item, isPending, error } = useItem(id)
  const children = useChildren(id)
  const play = usePlay()
  const setPlayed = useSetPlayed()
  const setFavorite = useSetFavorite()
  const quality = useQualityOverride() ?? undefined

  if (error) return <p className="p-6 text-sm text-destructive">{error.message}</p>
  if (isPending) return <Skeleton className="m-6 h-64 rounded-lg" />

  const resumable = item.positionTicks > 0
  const runtime = formatRuntime(item.runtimeTicks)

  return (
    <div className="flex flex-col gap-8 p-6">
      <div className="flex gap-6">
        {item.primaryImageTag && (
          <img
            src={imageUrl(item)}
            alt=""
            className="h-poster-h w-poster-w shrink-0 rounded-lg object-cover"
          />
        )}
        <div className="flex min-w-0 flex-col gap-3">
          <h1 className="text-2xl font-medium">{item.name}</h1>
          <div className="flex flex-wrap gap-2 text-sm text-muted-foreground">
            {[item.year, runtime, item.officialRating, item.genres?.join(", ")]
              .filter(Boolean)
              .map((part, index) => (
                <span key={index}>{part}</span>
              ))}
          </div>
          {item.overview && <p className="max-w-2xl text-sm leading-relaxed">{item.overview}</p>}
          <div className="flex flex-wrap items-center gap-2">
            <Button onClick={() => play.mutate({ id: item.id, resume: resumable, quality })}>
              <Play className="size-4" />
              {resumable ? "Resume" : "Play"}
            </Button>
            {resumable && (
              <Button
                variant="secondary"
                onClick={() => play.mutate({ id: item.id, resume: false, quality })}
              >
                Play from start
              </Button>
            )}
            <Button
              variant="secondary"
              onClick={() => setPlayed.mutate({ id: item.id, played: !item.played })}
            >
              <Check className="size-4" />
              {item.played ? "Mark unwatched" : "Mark watched"}
            </Button>
            <Button
              variant="secondary"
              onClick={() => setFavorite.mutate({ id: item.id, favorite: !item.favorite })}
            >
              <Heart className={item.favorite ? "size-4 fill-current" : "size-4"} />
            </Button>
            {item.kind !== "Series" && item.kind !== "Season" && <QualityPicker />}
          </div>
        </div>
      </div>

      {children.data && children.data.items.length > 0 && (
        <section className="flex flex-col gap-3">
          <h2 className="text-base font-medium">
            {item.kind === "Series" ? "Seasons" : "Episodes"}
          </h2>
          <div className="flex flex-wrap gap-[var(--card-gap)]">
            {children.data.items.map((child) => (
              <MediaCard key={child.id} item={child} />
            ))}
          </div>
        </section>
      )}
    </div>
  )
}
