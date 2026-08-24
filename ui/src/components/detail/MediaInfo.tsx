import { AudioLines, Captions, FileVideo, Film } from "lucide-react"
import { useState, type ReactNode } from "react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import type {
  MediaSource,
  MediaStream,
  PlaybackTrackPreference,
  PlaybackTrackPreferenceWrite,
} from "@/lib/api"
import {
  describeStream,
  describeTrackChoice,
  formatBitrate,
  formatCodec,
  formatFileSize,
  formatResolution,
  formatVideoRange,
} from "@/lib/format"
import { useSetPlaybackPreference } from "@/lib/queries"

/**
 * A full-subtitle release carries thirty tracks. Listing them all buries the
 * video and audio next to it, so the tail is one click away instead.
 */
const VISIBLE_STREAMS = 6

function StreamGroup({
  icon,
  title,
  streams,
  kind,
}: {
  icon: ReactNode
  title: string
  streams: MediaStream[]
  kind: "video" | "audio" | "subtitle"
}) {
  const [expanded, setExpanded] = useState(false)
  if (!streams.length) return null

  const hidden = expanded ? 0 : Math.max(0, streams.length - VISIBLE_STREAMS)
  const shown = hidden ? streams.slice(0, VISIBLE_STREAMS) : streams

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
        {icon}
        {title}
        {streams.length > 1 && <span className="tabular-nums">({streams.length})</span>}
      </div>
      <ul className="flex flex-col gap-1">
        {shown.map((stream) => (
          <li key={stream.index} className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
            <span className="min-w-0">{describeStream(stream, kind)}</span>
            {stream.isDefault && (
              <Badge variant="outline" className="border-border font-normal">
                Default
              </Badge>
            )}
          </li>
        ))}
      </ul>
      {hidden > 0 && (
        <Button
          variant="link"
          size="xs"
          className="self-start px-0"
          onClick={() => setExpanded(true)}
        >
          Show {hidden} more
        </Button>
      )}
    </div>
  )
}

function SourceCard({ source, selected }: { source: MediaSource; selected: boolean }) {
  const video = source.video[0]
  const headline = [
    video ? formatResolution(video) : null,
    video ? formatVideoRange(video) : null,
    video ? formatCodec(video.codec) : null,
    source.container?.toUpperCase(),
    formatFileSize(source.size),
    formatBitrate(source.bitrate),
  ].filter(Boolean)
  // The server only hands the real path to admins; for everyone else the source
  // name is the release name, which answers the same question.
  const label = source.fileName ?? source.name

  return (
    <div
      className={`flex flex-col gap-4 rounded-xl border bg-card p-4 shadow-lg shadow-black/40 ${
        selected ? "border-primary/35" : "border-border/60"
      }`}
    >
      {headline.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          {headline.map((part) => (
            <Badge key={part} variant="secondary" className="font-normal">
              {part}
            </Badge>
          ))}
        </div>
      )}
      {label && (
        <div className="flex items-start gap-2 text-xs break-all text-muted-foreground">
          <FileVideo className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {label}
        </div>
      )}
      <div className="grid gap-4 sm:grid-cols-3">
        <StreamGroup
          icon={<Film className="size-3.5" aria-hidden />}
          title="Video"
          streams={source.video}
          kind="video"
        />
        <StreamGroup
          icon={<AudioLines className="size-3.5" aria-hidden />}
          title="Audio"
          streams={source.audio}
          kind="audio"
        />
        <StreamGroup
          icon={<Captions className="size-3.5" aria-hidden />}
          title="Subtitles"
          streams={source.subtitles}
          kind="subtitle"
        />
      </div>
    </div>
  )
}

const SUBTITLES_OFF = "__off__"

function mediaSourceLabel(source: MediaSource, index: number) {
  return source.fileName?.trim() || source.name.trim() || `Source ${index + 1}`
}

function defaultAudioIndex(source: MediaSource) {
  return (
    source.audio.find((stream) => stream.index === source.defaultAudioStreamIndex)?.index ??
    source.audio.find((stream) => stream.isDefault)?.index ??
    source.audio[0]?.index ??
    null
  )
}

function defaultSubtitleIndex(source: MediaSource) {
  return (
    source.subtitles.find((stream) => stream.index === source.defaultSubtitleStreamIndex)?.index ??
    null
  )
}

function PlaybackTrackControls({
  sources,
  preference,
  save,
}: {
  sources: MediaSource[]
  preference: PlaybackTrackPreference | null
  save: PlaybackPreferenceWriter
}) {
  const sourceIndex = Math.min(preference?.mediaSourceIndex ?? 0, sources.length - 1)
  const source = sources[sourceIndex]
  if (!source) return null

  const hasControls = sources.length > 1 || source.audio.length > 1 || source.subtitles.length > 0
  if (!hasControls) return null

  const write = (
    selectedSourceIndex: number,
    audioStreamIndex: number | null,
    subtitleStreamIndex: number | null,
  ) => {
    const selectedSource = sources[selectedSourceIndex]
    if (!selectedSource) return
    save.mutate({
      mediaSourceId: selectedSource.id,
      mediaSourceIndex: selectedSourceIndex,
      audioStreamIndex,
      subtitleStreamIndex,
    })
  }

  return (
    <div className="grid gap-3 rounded-xl border border-primary/15 bg-card p-4 shadow-lg shadow-black/40 sm:grid-cols-3">
      {sources.length > 1 && (
        <div className="flex min-w-0 flex-col gap-1.5">
          <Label htmlFor="media-source-select">Media source</Label>
          <Select
            value={String(sourceIndex)}
            disabled={save.isPending}
            onValueChange={(value) => {
              const nextIndex = Number(value)
              const next = sources[nextIndex]
              if (!next) return
              write(nextIndex, defaultAudioIndex(next), defaultSubtitleIndex(next))
            }}
          >
            <SelectTrigger id="media-source-select" className="w-full" aria-label="Media source">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sources.map((candidate, index) => (
                <SelectItem key={candidate.id ?? index} value={String(index)}>
                  {mediaSourceLabel(candidate, index)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {source.audio.length > 1 && (
        <div className="flex min-w-0 flex-col gap-1.5">
          <Label htmlFor="audio-track-select">Audio</Label>
          <Select
            value={String(preference?.audioStreamIndex ?? defaultAudioIndex(source))}
            disabled={save.isPending}
            onValueChange={(value) =>
              write(sourceIndex, Number(value), preference?.subtitleStreamIndex ?? null)
            }
          >
            <SelectTrigger id="audio-track-select" className="w-full" aria-label="Audio track">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {source.audio.map((track) => (
                <SelectItem key={track.index} value={String(track.index)}>
                  {describeTrackChoice(track, "audio")}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {source.subtitles.length > 0 && (
        <div className="flex min-w-0 flex-col gap-1.5">
          <Label htmlFor="subtitle-track-select">Subtitles</Label>
          <Select
            value={
              preference?.subtitleStreamIndex == null
                ? SUBTITLES_OFF
                : String(preference.subtitleStreamIndex)
            }
            disabled={save.isPending}
            onValueChange={(value) =>
              write(
                sourceIndex,
                preference?.audioStreamIndex ?? defaultAudioIndex(source),
                value === SUBTITLES_OFF ? null : Number(value),
              )
            }
          >
            <SelectTrigger
              id="subtitle-track-select"
              className="w-full"
              aria-label="Subtitle track"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={SUBTITLES_OFF}>Subtitles off</SelectItem>
              {source.subtitles.map((track) => (
                <SelectItem key={track.index} value={String(track.index)}>
                  {describeTrackChoice(track, "subtitle")}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}
    </div>
  )
}

/**
 * What the file actually is: container, size, and every track in it.
 *
 * This is the one part of the page that always costs a request to the server —
 * codecs are not in the local cache — so it renders a skeleton rather than
 * holding the rest of the page back, and simply disappears when the server
 * cannot answer. Playback can still use server defaults without this panel;
 * when it does answer, the same current metadata validates saved selections.
 */
export function MediaInfo({
  itemId,
  sources,
  preference,
  isPending,
}: {
  itemId: string
  sources: MediaSource[] | undefined
  preference: PlaybackTrackPreference | null | undefined
  isPending: boolean
}) {
  const save = useSetPlaybackPreference(itemId)
  return (
    <MediaInfoView
      sources={sources}
      preference={preference}
      isPending={isPending}
      save={save}
    />
  )
}

export interface PlaybackPreferenceWriter {
  isPending: boolean
  mutate: (preference: PlaybackTrackPreferenceWrite) => void
}

export function MediaInfoView({
  sources,
  preference,
  isPending,
  save,
}: {
  sources: MediaSource[] | undefined
  preference: PlaybackTrackPreference | null | undefined
  isPending: boolean
  save: PlaybackPreferenceWriter
}) {
  if (isPending) {
    return (
      <section className="flex flex-col gap-3">
        <h2 className="section-title">Media</h2>
        <Skeleton className="h-40 rounded-lg" />
      </section>
    )
  }
  if (!sources?.length) return null

  return (
    <section className="flex flex-col gap-3">
      <h2 className="section-title">Media</h2>
      <div className="flex flex-col gap-3">
        <PlaybackTrackControls
          sources={sources}
          preference={preference ?? null}
          save={save}
        />
        {sources.map((source, index) => (
          <SourceCard
            key={source.id ?? index}
            source={source}
            selected={sources.length > 1 && index === (preference?.mediaSourceIndex ?? 0)}
          />
        ))}
      </div>
    </section>
  )
}
