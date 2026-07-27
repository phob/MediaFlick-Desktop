import { AudioLines, Captions, FileVideo, Film } from "lucide-react"
import { useState, type ReactNode } from "react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import type { MediaSource, MediaStream } from "@/lib/api"
import {
  describeStream,
  formatBitrate,
  formatCodec,
  formatFileSize,
  formatResolution,
  formatVideoRange,
} from "@/lib/format"

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

function SourceCard({ source }: { source: MediaSource }) {
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
    <div className="flex flex-col gap-4 rounded-xl border border-white/5 bg-card/55 p-4 shadow-lg shadow-black/10">
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

/**
 * What the file actually is: container, size, and every track in it.
 *
 * This is the one part of the page that always costs a request to the server —
 * codecs are not in the local cache — so it renders a skeleton rather than
 * holding the rest of the page back, and simply disappears when the server
 * cannot answer. Nothing here is needed to play the item.
 */
export function MediaInfo({
  sources,
  isPending,
}: {
  sources: MediaSource[] | undefined
  isPending: boolean
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
        {sources.map((source, index) => (
          <SourceCard key={source.id ?? index} source={source} />
        ))}
      </div>
    </section>
  )
}
