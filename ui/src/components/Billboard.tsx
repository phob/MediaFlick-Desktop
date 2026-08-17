import { Check, Info, Play, Plus, Star } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { Button } from "@/components/ui/button"
import {
  BACKDROP_WIDTH,
  landscapeImageCandidates,
  logoUrl,
  progressFraction,
  api,
  type ItemSummary,
} from "@/lib/api"
import { formatCommunityRating, formatRemaining, formatRuntime } from "@/lib/format"
import { isJsonObject, jsonNumber, jsonString, type JsonValue } from "@/lib/json"
import {
  useItemSynopsis,
  useNextUp,
  usePlay,
  useSetFavorite,
  useSettings,
  useTrailer,
} from "@/lib/queries"
import { usePrefersReducedMotion } from "@/lib/reduced-motion"
import { detailNavigationState } from "@/lib/navigation"
import { cn } from "@/lib/utils"

/** How long a title without a usable trailer holds before the next takes over. */
const NO_TRAILER_DWELL_MS = 12_000
/** Let the still establish the title before its local trailer takes over. */
const TRAILER_DELAY_MS = 5_000
/** A broken remote player must not strand the billboard forever. */
const REMOTE_TRAILER_WATCHDOG_MS = 3 * 60_000
const YOUTUBE_ORIGINS = new Set(["https://www.youtube-nocookie.com", "https://www.youtube.com"])

type YouTubeOutboundMessage =
  | { event: "listening" }
  | { event: "command"; func: "addEventListener"; args: ["onStateChange" | "onError"] }

interface YouTubeInboundMessage {
  event: string | null
  state: number | null
}

function readYouTubeMessage(data: JsonValue): YouTubeInboundMessage | null {
  let parsed = data
  const serialized = jsonString(data)
  if (serialized !== null) {
    try {
      parsed = JSON.parse(serialized)
    } catch {
      return null
    }
  }
  if (!isJsonObject(parsed)) return null
  const info = jsonNumber(parsed.info)
  const nestedInfo = isJsonObject(parsed.info) ? jsonNumber(parsed.info.playerState) : null
  return {
    event: jsonString(parsed.event),
    state: info ?? nestedInfo,
  }
}

function episodeLabel(item: ItemSummary) {
  if (item.kind !== "Episode") return null
  const season = item.parentIndexNumber
  const episode = item.indexNumber
  const code = season != null && episode != null ? `S${season} E${episode}` : null
  return [code, item.name].filter(Boolean).join(" · ")
}

/**
 * The home page's billboard: a rotating stack of full-bleed backdrops with one
 * title's copy over them.
 *
 * Only the slides that have actually been shown are mounted. A billboard of
 * five titles is five backdrops at `BACKDROP_WIDTH`, and mounting them all up
 * front would spend that bandwidth before the first one has even painted —
 * `opacity: 0` does not stop an `<img>` from being fetched. Growing the stack as
 * the rotation reaches each slide spreads that cost across the current title's
 * dwell or trailer instead.
 */
export function Billboard({ items }: { items: ItemSummary[] }) {
  const systemReducedMotion = usePrefersReducedMotion()
  const { data: settings } = useSettings()
  // Do not start automatic movement until the saved setting is known. Once it
  // is, either the in-app preference or the operating system can veto motion.
  const reducedMotion = systemReducedMotion || settings?.appearance.reducedMotion !== false
  const [paused, setPaused] = useState(false)
  const [index, setIndex] = useState(0)
  const [advancePending, setAdvancePending] = useState(false)
  const [mounted, setMounted] = useState<Set<number>>(() => new Set([0]))

  // A shorter list must not strand the billboard on an index that no longer
  // exists.
  useEffect(() => {
    setIndex((current) => (current < items.length ? current : 0))
  }, [items.length])

  useEffect(() => {
    setMounted((previous) => (previous.has(index) ? previous : new Set(previous).add(index)))
  }, [index])

  // Playback completion can happen while the pointer or keyboard focus is in
  // the hero. Remember it, then advance once interacting with the hero is no
  // longer able to make a button change underneath the user.
  useEffect(() => {
    if (!advancePending || paused || reducedMotion || items.length <= 1) return
    setAdvancePending(false)
    setIndex((current) => (current + 1) % items.length)
  }, [advancePending, items.length, paused, reducedMotion])

  const current = items[index]
  const currentId = current?.id
  const currentIdRef = useRef(currentId)
  currentIdRef.current = currentId
  const finishCurrent = useCallback(
    (itemId: string) => {
      if (currentIdRef.current === itemId) setAdvancePending(true)
    },
    [],
  )
  const selectSlide = (slide: number) => {
    setAdvancePending(false)
    setIndex(slide)
  }
  // The synopsis is live-only and arrives after the cached hero has painted;
  // until it lands (or offline) the billboard simply shows no overview.
  const synopsis = useItemSynopsis(current?.id)
  const nextUp = useNextUp(current?.id, current?.kind === "Series")
  if (!current) return null

  return (
    <section
      className="relative flex h-1/2 min-h-[30rem] shrink-0 items-end overflow-hidden"
      aria-labelledby="billboard-title"
      // Pausing while the pointer is in the hero is what makes the action
      // buttons usable: a rotation underneath a reaching cursor swaps the
      // button's meaning out from under the click.
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={() => setPaused(false)}
    >
      {items.map((item, slide) =>
        mounted.has(slide) ? (
          <BillboardBackdrop
            key={item.id}
            item={item}
            active={slide === index}
            reducedMotion={reducedMotion}
            onFinished={finishCurrent}
          />
        ) : null,
      )}

      <BillboardCopy
        key={current.id}
        item={current}
        overview={synopsis.data?.overview ?? null}
        summary={current}
        playTarget={current.kind === "Series" ? nextUp.data?.item : current}
      />

      {items.length > 1 && (
        <div className="absolute right-6 bottom-28 z-20 flex items-center gap-3 sm:right-10 lg:right-14">
          <span className="data-label text-muted-foreground">
            {String(index + 1).padStart(2, "0")}
            <span className="mx-1 text-primary/60">/</span>
            {String(items.length).padStart(2, "0")}
          </span>
          <div className="flex items-center gap-1.5">
            {items.map((item, slide) => (
              <button
                key={item.id}
                type="button"
                aria-label={`Show ${item.name}`}
                aria-current={slide === index}
                onClick={() => selectSlide(slide)}
                data-active={slide === index}
                className="billboard-tick focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              />
            ))}
          </div>
        </div>
      )}
    </section>
  )
}

/**
 * One slide's artwork. Both gradients are on this layer rather than over the
 * whole stack so that a fading slide takes its own scrim with it — a shared
 * scrim would sit at full strength over a backdrop that is half gone.
 */
function BillboardBackdrop({
  item,
  active,
  reducedMotion,
  onFinished,
}: {
  item: ItemSummary
  active: boolean
  reducedMotion: boolean
  onFinished: (itemId: string) => void
}) {
  const images = landscapeImageCandidates(item, BACKDROP_WIDTH)
  const [imageIndex, setImageIndex] = useState(0)
  const [trailerArmed, setTrailerArmed] = useState(false)
  const [trailerPlaying, setTrailerPlaying] = useState(false)
  const [youtubeLoaded, setYoutubeLoaded] = useState(false)
  const youtubeFrame = useRef<HTMLIFrameElement>(null)
  const finished = useRef(false)
  const trailer = useTrailer(item.id, active && !reducedMotion)
  const image = images[imageIndex]

  const complete = useCallback(() => {
    if (finished.current) return
    finished.current = true
    setTrailerPlaying(false)
    setTrailerArmed(false)
    onFinished(item.id)
  }, [item.id, onFinished])

  useEffect(() => {
    finished.current = false
    setTrailerArmed(false)
    setTrailerPlaying(false)
    setYoutubeLoaded(false)
    if (!active || reducedMotion) return
    const timer = window.setTimeout(() => setTrailerArmed(true), TRAILER_DELAY_MS)
    return () => window.clearTimeout(timer)
  }, [active, item.id, reducedMotion])

  const trailerInfo = trailer.data?.trailer
  const trailerId = trailerInfo?.id
  const trailerEmbed = trailerInfo?.embedUrl

  // Keep ordinary stills rotating. A real trailer takes ownership of the
  // lifecycle below and does not release the slide until playback completes.
  useEffect(() => {
    if (
      !active ||
      reducedMotion ||
      (!trailer.isSuccess && !trailer.isError) ||
      trailerId ||
      trailerEmbed
    ) {
      return
    }
    const timer = window.setTimeout(complete, NO_TRAILER_DWELL_MS)
    return () => window.clearTimeout(timer)
  }, [
    active,
    complete,
    reducedMotion,
    trailer.isError,
    trailer.isSuccess,
    trailerEmbed,
    trailerId,
  ])

  // YouTube's embedded player reports state 0 when the video ends. Listen only
  // to the exact iframe window and its two documented origins; no remote script
  // is loaded into the privileged app page.
  useEffect(() => {
    const frame = youtubeFrame.current
    if (!active || !trailerArmed || !trailerEmbed || !frame) return
    const playerId = `billboard-youtube-${item.id}`
    const post = (message: YouTubeOutboundMessage) =>
      frame.contentWindow?.postMessage(
        JSON.stringify({ ...message, id: playerId }),
        "https://www.youtube-nocookie.com",
      )
    const subscribe = () => {
      post({ event: "listening" })
      post({ event: "command", func: "addEventListener", args: ["onStateChange"] })
      post({ event: "command", func: "addEventListener", args: ["onError"] })
    }
    const handleMessage = (event: MessageEvent) => {
      if (event.source !== frame.contentWindow || !YOUTUBE_ORIGINS.has(event.origin)) return
      const message = readYouTubeMessage(event.data)
      if (message?.state === 1) setTrailerPlaying(true)
      if (message?.state === 0 || message?.event === "onError") complete()
    }

    window.addEventListener("message", handleMessage)
    subscribe()
    const subscribeTimer = window.setInterval(subscribe, 1_000)
    const stopSubscribing = window.setTimeout(
      () => window.clearInterval(subscribeTimer),
      10_000,
    )
    return () => {
      window.removeEventListener("message", handleMessage)
      window.clearInterval(subscribeTimer)
      window.clearTimeout(stopSubscribing)
    }
  }, [active, complete, item.id, trailerArmed, trailerEmbed])

  // The raw state messages are deliberately a progressive enhancement. Reveal
  // a loaded player even if a particular YouTube build does not acknowledge
  // the listener, but retain a long watchdog so a broken embed cannot pin the
  // home screen forever.
  useEffect(() => {
    if (!active || !trailerArmed || !trailerEmbed || !youtubeLoaded) return
    const reveal = window.setTimeout(() => setTrailerPlaying(true), 600)
    const watchdog = window.setTimeout(complete, REMOTE_TRAILER_WATCHDOG_MS)
    return () => {
      window.clearTimeout(reveal)
      window.clearTimeout(watchdog)
    }
  }, [active, complete, trailerArmed, trailerEmbed, youtubeLoaded])

  if (!image) return null

  return (
    <div
      aria-hidden
      data-trailer-playing={trailerPlaying}
      data-trailer-source={trailerId ? "local" : trailerEmbed ? "youtube" : undefined}
      className={cn(
        "billboard-slide pointer-events-none absolute inset-0",
        active ? "opacity-100" : "opacity-0",
      )}
    >
      <img
        src={image}
        alt=""
        decoding="async"
        onError={() => setImageIndex((current) => current + 1)}
        className="billboard-backdrop-image media-backdrop-image"
      />
      {trailerArmed && trailerId && (
        <div
          className="billboard-trailer-stage"
          data-playing={trailerPlaying}
          data-source="local"
        >
          <div className="billboard-trailer-feather">
            <video
              key={trailerId}
              aria-hidden
              autoPlay
              muted
              playsInline
              preload="auto"
              src={api.trailerStreamUrl(trailerId)}
              onPlaying={() => setTrailerPlaying(true)}
              onEnded={complete}
              onError={complete}
              className="billboard-trailer-local"
            />
          </div>
        </div>
      )}
      {trailerArmed && trailerEmbed && (
        <div
          className="billboard-trailer-stage"
          data-playing={trailerPlaying}
          data-source="youtube"
        >
          <div className="billboard-trailer-feather">
            <iframe
              key={trailerEmbed}
              ref={youtubeFrame}
              id={`billboard-youtube-${item.id}`}
              aria-hidden
              title=""
              tabIndex={-1}
              allow="autoplay; encrypted-media"
              sandbox="allow-scripts allow-same-origin allow-presentation"
              src={trailerEmbed}
              onLoad={() => setYoutubeLoaded(true)}
              className="billboard-trailer-youtube"
            />
          </div>
        </div>
      )}
      <div className="billboard-media-vignette absolute inset-0" />
      <div className="absolute inset-0 bg-linear-to-r from-background from-4% via-background/82 via-42% to-background/12" />
      <div className="absolute inset-0 bg-linear-to-t from-background from-1% via-transparent via-52% to-background/20" />
    </div>
  )
}

function BillboardCopy({
  item,
  overview,
  summary,
  playTarget,
}: {
  item: ItemSummary
  /** Fetched live after the hero paints; null until then. */
  overview: string | null
  /** Always the row's own entry — what the buttons act on. */
  summary: ItemSummary
  playTarget?: ItemSummary | null
}) {
  const location = useLocation()
  const play = usePlay()
  const setFavorite = useSetFavorite()
  const [logoFailed, setLogoFailed] = useState(false)

  const progress = progressFraction(item)
  const remaining = formatRemaining(item.positionTicks, item.runtimeTicks)
  const isEpisode = item.kind === "Episode"
  const target = item.kind === "Series" ? playTarget : summary
  const isPlayable = Boolean(target && (target.kind === "Movie" || target.kind === "Episode"))
  const title = isEpisode && item.seriesName ? item.seriesName : item.name
  const secondaryTitle = episodeLabel(item)
  const communityRating = formatCommunityRating(item.communityRating)
  const logo = logoUrl(item)
  const kicker =
    item.positionTicks > 0 ? "Continue watching" : item.played ? "Watch it again" : "Featured"

  return (
    // The copy hangs off a lit rule rather than floating over the artwork
    // unanchored — the shelf marker's gesture, at hero scale.
    <div className="billboard-copy relative z-10 flex max-w-2xl gap-5 px-6 pb-24 sm:px-10 sm:pb-28 lg:px-14">
      <span className="billboard-spine" aria-hidden />
      <div className="flex min-w-0 flex-col items-start gap-4 py-1">
      <p className="data-label text-primary">{kicker}</p>

      <div className="flex flex-col gap-2">
        {/* The heading is always in the document — a wordmark is artwork, and a
            page whose <h1> is an image is a page with no title. When the logo
            loads it takes the heading's place visually and the text stays for
            anything reading the page rather than looking at it. */}
        <h1 id="billboard-title" className={cn(logo && !logoFailed && "sr-only")}>
          <span className="block max-w-xl text-4xl leading-[0.96] font-black tracking-[-0.04em] text-balance drop-shadow-lg sm:text-5xl lg:text-6xl">
            {title}
          </span>
        </h1>
        {logo && !logoFailed && (
          <img
            src={logo}
            alt=""
            decoding="async"
            onError={() => setLogoFailed(true)}
            className="max-h-20 w-auto max-w-sm object-contain object-left drop-shadow-2xl sm:max-h-24"
          />
        )}
        {secondaryTitle && (
          <p className="data-value text-foreground/85 sm:text-sm">{secondaryTitle}</p>
        )}
      </div>

      {/* Facts, in the data face and divided by accent slashes. Titles are
          typeset; everything known about them is measured. */}
      <div className="data-value flex flex-wrap items-center gap-x-2 gap-y-2 text-foreground/80">
        {communityRating && (
          <span
            className="inline-flex items-center gap-1 font-semibold text-primary"
            title={`Community rating: ${communityRating}/10`}
          >
            <Star className="size-3.5 fill-current" aria-hidden />
            {communityRating}
          </span>
        )}
        {[
          item.year != null ? String(item.year) : null,
          formatRuntime(item.runtimeTicks),
        ]
          .filter(Boolean)
          .map((fact) => (
            <span key={fact} className="flex items-center gap-2">
              <span className="text-primary/45" aria-hidden>
                /
              </span>
              {fact}
            </span>
          ))}
        {/* No "HD" plaque alongside it: nothing at this level knows the
            resolution, and the media info on the detail page does. */}
        {item.officialRating && (
          <span className="data-label border border-foreground/25 px-1.5 py-0.5 text-foreground/75">
            {item.officialRating}
          </span>
        )}
      </div>

      {overview && (
        <p className="line-clamp-3 max-w-xl text-sm leading-relaxed text-foreground/85 drop-shadow sm:text-base">
          {overview}
        </p>
      )}

      {progress > 0 && (
        <div className="flex w-full max-w-md items-center gap-3">
          <div className="h-[3px] flex-1 overflow-hidden bg-foreground/20">
            <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
          </div>
          {/* Not the uppercase label face: "16m left" set in caps reads as
              "16M LEFT", which looks like a size rather than a duration. */}
          {remaining && <span className="data-value shrink-0 text-foreground/70">{remaining}</span>}
        </div>
      )}

      {/* The accent carries the primary action rather than white doing it. */}
      <div className="flex flex-wrap items-center gap-2 pt-1">
        {isPlayable && (
          <Button
            size="lg"
            disabled={play.isPending}
            className="min-w-28 shadow-lg shadow-primary/20 hover:bg-primary/85"
            onClick={() => play.mutate({ id: target!.id, resume: target!.positionTicks > 0 })}
          >
            <Play className="size-5 fill-current" />
            {target!.positionTicks > 0 ? "Resume" : "Play"}
          </Button>
        )}
        <Button
          variant="outline"
          size="lg"
          className="border-foreground/25 bg-background/50 backdrop-blur-sm hover:border-primary/60 hover:bg-background/70 hover:text-primary"
          asChild
        >
          <Link
            to={`/item/${encodeURIComponent(summary.id)}`}
            state={detailNavigationState(location)}
          >
            <Info className="size-5" />
            Details
          </Link>
        </Button>
        <Button
          variant="outline"
          size="icon-lg"
          className={cn(
            "border-foreground/25 bg-background/50 backdrop-blur-sm hover:border-primary/60 hover:bg-background/70 hover:text-primary",
            summary.favorite && "border-primary/70 text-primary",
          )}
          aria-label={summary.favorite ? "Remove from My List" : "Add to My List"}
          aria-pressed={summary.favorite}
          disabled={setFavorite.isPending}
          onClick={() => setFavorite.mutate({ id: summary.id, favorite: !summary.favorite })}
        >
          {summary.favorite ? <Check className="size-5" /> : <Plus className="size-5" />}
        </Button>
      </div>
      </div>
    </div>
  )
}
