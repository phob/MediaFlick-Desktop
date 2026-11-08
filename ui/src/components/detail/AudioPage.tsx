import { AudioLines, Play, RotateCcw, Shuffle } from "lucide-react"
import { DetailHeroLayout, type HeroArtworkAspect } from "@/components/detail/DetailHeroLayout"
import { DetailBackdrop } from "@/components/detail/DetailPrimitives"
import { formatClock } from "@/components/player/format"
import { Button } from "@/components/ui/button"
import { Pressable } from "@/components/ui/pressable"
import {
  backdropUrl,
  DETAIL_POSTER_WIDTH,
  imageUrl,
  TICKS_PER_MS,
  type AudioDisplay,
  type AudioItemDetail,
} from "@/lib/api"
import { artworkShape, type ArtworkShape } from "@/lib/artwork"
import { formatRuntime, formatYearOnly } from "@/lib/format"
import type { DetailNavigationState } from "@/lib/navigation"
import { usePlay, usePlayerState, usePlayQueue } from "@/lib/queries"
import { cn } from "@/lib/utils"

const HERO_FRAME: Record<ArtworkShape, HeroArtworkAspect> = {
  portrait: "poster",
  square: "square",
  landscape: "still",
}

/** An album, audiobook or track: Play and Shuffle, or Continue and From start, over its tracks by disc. */
export function AudioPage({
  item,
  navigationState,
}: {
  item: AudioItemDetail
  navigationState: DetailNavigationState
}) {
  const playQueue = usePlayQueue()
  const play = usePlay()
  const { data: player } = usePlayerState()
  const nowPlaying = player?.active && player.nowPlaying?.kind === "audio" ? player.nowPlaying : null
  const tracks = item.discs.flatMap((disc) => disc.tracks)
  const totalTicks = tracks.reduce((sum, track) => sum + (track.runtimeTicks ?? 0), 0)
  const audiobook = item.mediaKind === "audiobook"
  const resumable = audiobook && item.positionTicks > 0 && !item.played
  const pending = playQueue.isPending || play.isPending
  const albumArtists = item.albumArtists.length > 0
    ? item.albumArtists
    : [...new Set(tracks.flatMap((track) => track.artists))]
  const facts = [
    item.year != null ? String(item.year) : formatYearOnly(item.premiereDate),
    tracks.length > 1 ? `${tracks.length} tracks` : null,
    formatRuntime(totalTicks || item.runtimeTicks),
  ].filter((fact): fact is string => Boolean(fact))
  const backdrop = backdropUrl(item)

  return (
    <div className="detail-page relative isolate flex min-h-full min-w-0 flex-col gap-10 pb-16">
      {backdrop && <DetailBackdrop src={backdrop} />}
      <DetailHeroLayout
        compact
        back={{ to: navigationState.from, label: navigationState.label }}
        poster={item.primaryImageTag
          ? {
              src: imageUrl(item, "Primary", DETAIL_POSTER_WIDTH),
              aspect: HERO_FRAME[artworkShape(item.primaryAspect)],
              fit: "contain",
              progress: resumable && item.runtimeTicks ? item.positionTicks / item.runtimeTicks : undefined,
            }
          : null}
        title={item.name}
        subtitle={albumArtists.length > 0
          ? <p className="text-lg text-muted-foreground">{albumArtists.join(", ")}</p>
          : undefined}
        facts={facts}
        genres={item.genres.map((genre) => ({ label: genre }))}
        overview={item.overview ? (
          <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">{item.overview}</p>
        ) : undefined}
      >
        <div className="flex flex-wrap items-center gap-3">
          {audiobook ? (
            <>
              <Button
                size="lg"
                className="min-w-28 shadow-lg shadow-primary/20 hover:bg-primary/85"
                disabled={pending}
                onClick={() => play.mutate({ id: item.id, resume: resumable })}
              >
                <Play className="size-4 fill-current" />
                {resumable ? `Continue at ${formatClock(item.positionTicks / TICKS_PER_MS)}` : "Play"}
              </Button>
              {resumable && (
                <Button
                  variant="secondary"
                  size="lg"
                  className="hover:text-primary"
                  disabled={pending}
                  onClick={() => play.mutate({ id: item.id, resume: false })}
                >
                  <RotateCcw className="size-4" />
                  From start
                </Button>
              )}
            </>
          ) : (
            <>
              <Button
                size="lg"
                className="min-w-28 shadow-lg shadow-primary/20 hover:bg-primary/85"
                disabled={pending || tracks.length === 0}
                onClick={() => playQueue.mutate({ parentId: item.id })}
              >
                <Play className="size-4 fill-current" />
                Play
              </Button>
              {tracks.length > 1 && (
                <Button
                  variant="secondary"
                  size="lg"
                  className="hover:text-primary"
                  disabled={pending}
                  onClick={() => playQueue.mutate({ parentId: item.id, shuffle: true })}
                >
                  <Shuffle className="size-4" />
                  Shuffle
                </Button>
              )}
            </>
          )}
        </div>
      </DetailHeroLayout>

      {tracks.length > 0 && (
        <section aria-label="Tracks" className="flex max-w-5xl flex-col gap-6 px-6 sm:px-10 lg:px-14">
          {item.discs.map((disc) => (
            <div key={disc.number} className="flex flex-col gap-1">
              {item.discs.length > 1 && <h2 className="section-title mb-2">Disc {disc.number}</h2>}
              <ol aria-label={item.discs.length > 1 ? `Disc ${disc.number}` : undefined} className="flex flex-col">
                {disc.tracks.map((track, index) => (
                  <li key={track.itemId}>
                    <TrackRow
                      track={track}
                      position={track.trackNumber ?? index + 1}
                      albumArtists={albumArtists}
                      current={nowPlaying?.current.itemId === track.itemId}
                      disabled={pending}
                      onPlay={() => audiobook
                        ? play.mutate({ id: track.itemId, resume: resumable })
                        : playQueue.mutate({ parentId: item.id, startItemId: track.itemId })}
                    />
                  </li>
                ))}
              </ol>
            </div>
          ))}
        </section>
      )}
    </div>
  )
}

function TrackRow({
  track,
  position,
  albumArtists,
  current,
  disabled,
  onPlay,
}: {
  track: AudioDisplay
  position: number
  albumArtists: string[]
  current: boolean
  disabled: boolean
  onPlay: () => void
}) {
  const ownArtists = track.artists.join(", ")
  const showArtists = ownArtists !== "" && ownArtists !== albumArtists.join(", ")
  return (
    <Pressable
      aria-label={`Play ${track.title}`}
      aria-current={current || undefined}
      disabled={disabled}
      onClick={onPlay}
      className={cn(
        "group flex w-full items-center gap-4 rounded-md px-3 py-2.5 text-left transition hover:bg-white/6 focus-visible:bg-white/6 focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-60",
        current && "bg-primary/10 text-primary",
      )}
    >
      <span className="flex w-6 shrink-0 justify-end text-sm tabular-nums text-muted-foreground">
        {current ? <AudioLines className="size-4 text-primary" aria-hidden /> : (
          <>
            <span className="group-hover:hidden group-focus-visible:hidden">{position}</span>
            <Play className="hidden size-4 fill-current group-hover:block group-focus-visible:block" aria-hidden />
          </>
        )}
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate font-medium">{track.title}</span>
        {showArtists && <span className="truncate text-xs text-muted-foreground">{ownArtists}</span>}
      </span>
      <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
        {track.runtimeTicks ? formatClock(track.runtimeTicks / TICKS_PER_MS) : ""}
      </span>
    </Pressable>
  )
}
