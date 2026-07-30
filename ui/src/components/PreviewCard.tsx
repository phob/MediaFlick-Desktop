import { Check, ChevronDown, Play, Plus, Star, ThumbsUp } from "lucide-react"
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { createPortal } from "react-dom"
import { useLocation, useNavigate } from "react-router-dom"
import {
  LANDSCAPE_WIDTH,
  landscapeImageCandidates,
  logoUrl,
  progressFraction,
  type ItemSummary,
} from "@/lib/api"
import { formatCommunityRating, formatRemaining, formatRuntime } from "@/lib/format"
import { PreviewContext, type PreviewApi, type PreviewTarget } from "@/lib/preview"
import { useItem, useNextUp, usePlay, useSetFavorite, useSetPlayed } from "@/lib/queries"
import { cn } from "@/lib/utils"

/**
 * How long the pointer has to rest on a card before it expands. Short enough to
 * feel like a property of the card, long enough that sweeping the pointer across
 * a rail on the way somewhere else does not set off a chain of them.
 */
const OPEN_DELAY = 550
/**
 * The grace period on the way out. The panel is drawn over the card it belongs
 * to, so travelling from one to the other necessarily leaves the card first;
 * without a delay the panel would close under the pointer that was reaching it.
 */
const CLOSE_DELAY = 140

const PANEL_WIDTH = 348
/** Only used before the panel has been measured, to pick a first position. */
const ESTIMATED_HEIGHT = 320
const VIEWPORT_MARGIN = 12

/**
 * The expanded-card layer. One panel exists for the whole app: it is positioned
 * over whichever card the pointer is resting on, rather than each card owning a
 * copy that only differs in where it sits.
 *
 * It lives in a portal because the rails clip their overflow — an expansion
 * drawn inside the scroller would be cut off at the row's edge, which is the
 * whole thing it exists to escape.
 */
export function PreviewProvider({ children }: { children: ReactNode }) {
  const [target, setTarget] = useState<PreviewTarget | null>(null)
  const openTimer = useRef<number | undefined>(undefined)
  const closeTimer = useRef<number | undefined>(undefined)
  const location = useLocation()

  const clearTimers = useCallback(() => {
    window.clearTimeout(openTimer.current)
    window.clearTimeout(closeTimer.current)
  }, [])

  const api = useMemo<PreviewApi>(
    () => ({
      open: (item, anchor) => {
        window.clearTimeout(openTimer.current)
        window.clearTimeout(closeTimer.current)
        openTimer.current = window.setTimeout(() => {
          // Re-read the rect at fire time rather than at hover time: the rail
          // may have been scrolled during the delay, and a stale rect would
          // open the panel over whatever slid into that spot.
          setTarget({ item, rect: anchor.getBoundingClientRect() })
        }, OPEN_DELAY)
      },
      cancel: () => window.clearTimeout(openTimer.current),
      release: () => {
        window.clearTimeout(openTimer.current)
        closeTimer.current = window.setTimeout(() => setTarget(null), CLOSE_DELAY)
      },
      hold: () => window.clearTimeout(closeTimer.current),
      activeId: target?.item.id ?? null,
    }),
    [target],
  )

  // Anything that moves the page moves the card out from under the panel, and
  // the panel has no way to follow: its position was resolved from a rect taken
  // once. Closing is the honest response — re-anchoring mid-scroll reads as the
  // panel chasing the pointer.
  useEffect(() => {
    if (!target) return
    const close = () => {
      clearTimers()
      setTarget(null)
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close()
    }
    // Capture: the rails and the content pane scroll, not the window, and a
    // scroll event from those does not bubble.
    window.addEventListener("scroll", close, true)
    window.addEventListener("resize", close)
    window.addEventListener("keydown", onKeyDown)
    return () => {
      window.removeEventListener("scroll", close, true)
      window.removeEventListener("resize", close)
      window.removeEventListener("keydown", onKeyDown)
    }
  }, [target, clearTimers])

  // Navigating away — which the panel's own More Info button does — has to take
  // the panel with it, or it outlives the row it was opened from.
  useEffect(() => {
    clearTimers()
    setTarget(null)
  }, [location.pathname, location.search, clearTimers])

  useEffect(() => clearTimers, [clearTimers])

  return (
    <PreviewContext.Provider value={api}>
      {children}
      {target &&
        createPortal(
          <PreviewPanel
            key={target.item.id}
            target={target}
            onHold={api.hold}
            onRelease={api.release}
          />,
          document.body,
        )}
    </PreviewContext.Provider>
  )
}

/** Where the panel sits: centred on the card, kept inside the viewport. */
function usePanelPosition(rect: DOMRect, height: number) {
  const clamp = (value: number, max: number) =>
    Math.max(VIEWPORT_MARGIN, Math.min(value, max - VIEWPORT_MARGIN))
  return {
    left: clamp(rect.left + rect.width / 2 - PANEL_WIDTH / 2, window.innerWidth - PANEL_WIDTH),
    top: clamp(rect.top + rect.height / 2 - height / 2, window.innerHeight - height),
  }
}

function PreviewPanel({
  target,
  onHold,
  onRelease,
}: {
  target: PreviewTarget
  onHold: () => void
  onRelease: () => void
}) {
  const { item, rect } = target
  const navigate = useNavigate()
  const panel = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(ESTIMATED_HEIGHT)

  // The panel's height depends on how much metadata the item turns out to have,
  // so it can only be centred once it exists. The first measurement happens in a
  // layout effect, before the browser paints, which is why the correction from
  // the estimate is never seen.
  //
  // An observer rather than a re-measure on every render: the height moves again
  // when the detail record lands and once more if a logo fails and gives way to
  // the typeset title, and neither of those is a value this could list as a
  // dependency.
  useLayoutEffect(() => {
    const node = panel.current
    if (!node) return
    const apply = () => {
      const measured = node.offsetHeight
      if (measured) setHeight((previous) => (previous === measured ? previous : measured))
    }
    apply()
    const observer = new ResizeObserver(apply)
    observer.observe(node)
    return () => observer.disconnect()
  }, [])

  const { left, top } = usePanelPosition(rect, height)

  // Everything past the summary — genres, the overview, the series' own logo —
  // lives on the detail record. It is fetched only once a card has actually
  // been rested on, and the cache keeps it for the next hover and for the
  // detail page the More Info button leads to.
  const { data: detail } = useItem(item.id)
  const isSeries = item.kind === "Series"
  const nextUp = useNextUp(item.id, isSeries)
  // The title treatment that belongs over an episode still is the *show's*, not
  // the episode's — episodes essentially never carry one of their own. Disabled
  // for everything else, so this costs a request only on a Continue Watching
  // card, and only the first time one is rested on.
  const series = useItem(item.kind === "Episode" ? (item.seriesId ?? undefined) : undefined)

  const play = usePlay()
  const setFavorite = useSetFavorite()
  const setPlayed = useSetPlayed()

  // A series is not playable itself; its button stands in for one episode.
  const playTarget = isSeries ? (nextUp.data?.item ?? null) : item
  const canPlay = Boolean(
    playTarget && (playTarget.kind === "Movie" || playTarget.kind === "Episode"),
  )
  const progress = progressFraction(item)
  const remaining = formatRemaining(item.positionTicks, item.runtimeTicks)
  const communityRating = formatCommunityRating(item.communityRating)
  const logo = logoUrl(item) ?? (series.data ? logoUrl(series.data) : null)

  const seasons =
    isSeries && item.childCount
      ? `${item.childCount} ${item.childCount === 1 ? "season" : "seasons"}`
      : null
  const facts = [
    item.year ? String(item.year) : null,
    seasons ?? formatRuntime(item.runtimeTicks),
  ].filter(Boolean)

  const goToDetail = () => navigate(`/item/${encodeURIComponent(item.id)}`)

  return (
    <div
      ref={panel}
      // Not a dialog: nothing here is modal and nothing traps focus. It is a
      // hover affordance over a link that already carries the same title, so it
      // stays out of the accessibility tree rather than announcing a second
      // copy of every card in the row.
      aria-hidden
      onPointerEnter={onHold}
      onPointerLeave={onRelease}
      style={{ left, top, width: PANEL_WIDTH }}
      className="preview-panel fixed z-50 overflow-hidden rounded-media bg-card shadow-2xl shadow-black/80 ring-1 ring-primary/25"
    >
      <PreviewArt item={item} logo={logo} progress={progress} remaining={remaining} />

      <div className="flex flex-col gap-3 p-4">
        <div className="flex items-center gap-2">
          {canPlay && (
            <button
              type="button"
              disabled={play.isPending}
              title={playTarget!.positionTicks > 0 ? "Resume" : "Play"}
              onClick={() =>
                play.mutate({ id: playTarget!.id, resume: playTarget!.positionTicks > 0 })
              }
              className="preview-action bg-primary text-primary-foreground hover:bg-primary/85"
            >
              <Play className="size-5 fill-current" />
            </button>
          )}
          <button
            type="button"
            disabled={setFavorite.isPending}
            title={item.favorite ? "Remove from My List" : "Add to My List"}
            onClick={() => setFavorite.mutate({ id: item.id, favorite: !item.favorite })}
            className={cn(
              "preview-action border",
              item.favorite
                ? "border-primary/70 text-primary"
                : "border-foreground/30 text-foreground/80 hover:border-primary/60 hover:text-primary",
            )}
          >
            {item.favorite ? <Check className="size-5" /> : <Plus className="size-5" />}
          </button>
          {/* Jellyfin has no separate "liked"; watched is the nearest real
              state, and the thumb is how this row is read at a glance. */}
          <button
            type="button"
            disabled={setPlayed.isPending}
            title={item.played ? "Mark as unwatched" : "Mark as watched"}
            onClick={() =>
              setPlayed.mutate({ id: item.id, played: !item.played, context: item.seriesId })
            }
            className={cn(
              "preview-action border",
              item.played
                ? "border-primary/70 bg-primary/15 text-primary"
                : "border-foreground/30 text-foreground/80 hover:border-primary/60 hover:text-primary",
            )}
          >
            <ThumbsUp className={cn("size-5", item.played && "fill-current")} />
          </button>
          <button
            type="button"
            title="More info"
            onClick={goToDetail}
            className="preview-action ml-auto border border-foreground/30 text-foreground/80 hover:border-primary/60 hover:text-primary"
          >
            <ChevronDown className="size-5" />
          </button>
        </div>

        <div className="data-value flex flex-wrap items-center gap-x-2 gap-y-1.5">
          {communityRating && (
            <span
              className="inline-flex items-center gap-1 font-semibold text-primary"
              title={`Community rating: ${communityRating}/10`}
            >
              <Star className="size-3.5 fill-current" aria-hidden />
              {communityRating}
            </span>
          )}
          {item.officialRating && (
            <span className="data-label border border-foreground/25 px-1.5 py-0.5 text-foreground/75">
              {item.officialRating}
            </span>
          )}
          {facts.map((fact) => (
            <span key={fact} className="flex items-center gap-2 text-foreground/75">
              <span className="text-primary/45" aria-hidden>
                /
              </span>
              {fact}
            </span>
          ))}
        </div>

        {detail?.genres && detail.genres.length > 0 && (
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-foreground/65">
            {detail.genres.slice(0, 3).map((genre, index) => (
              <span key={genre} className="flex items-center gap-2">
                {index > 0 && (
                  <span className="text-primary/40" aria-hidden>
                    ·
                  </span>
                )}
                {genre}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

/** The 16:9 head of the panel: artwork, the title over it, and the resume bar. */
function PreviewArt({
  item,
  logo,
  progress,
  remaining,
}: {
  item: ItemSummary
  logo: string | null
  progress: number
  remaining: string | null
}) {
  const images = landscapeImageCandidates(item, LANDSCAPE_WIDTH)
  const [imageIndex, setImageIndex] = useState(0)
  const [logoFailed, setLogoFailed] = useState(false)
  const image = images[imageIndex]
  const title = item.kind === "Episode" && item.seriesName ? item.seriesName : item.name

  return (
    <div className="relative aspect-video w-full overflow-hidden bg-secondary">
      {image && (
        <img
          src={image}
          alt=""
          decoding="async"
          onError={() => setImageIndex((current) => current + 1)}
          className="media-artwork-image h-full w-full object-cover"
        />
      )}
      <div className="absolute inset-0 bg-linear-to-t from-card via-card/25 to-transparent" />

      <div className="absolute inset-x-4 bottom-3 flex flex-col gap-1.5">
        {logo && !logoFailed ? (
          <img
            src={logo}
            alt={title}
            decoding="async"
            onError={() => setLogoFailed(true)}
            className="max-h-12 w-auto max-w-[65%] self-start object-contain drop-shadow-lg"
          />
        ) : (
          <p className="line-clamp-2 text-base leading-tight font-semibold drop-shadow-lg">
            {title}
          </p>
        )}
        {progress > 0 && (
          <div className="flex items-center gap-2">
            <div className="h-[3px] flex-1 overflow-hidden bg-foreground/25">
              <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
            </div>
            {remaining && (
              <span className="data-value shrink-0 text-[11px] text-foreground/70">{remaining}</span>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
