import { ChevronLeft, ChevronRight } from "lucide-react"
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react"
import { Link } from "react-router-dom"
import { cn } from "@/lib/utils"

/**
 * One horizontally scrolling shelf: a heading, an optional "Browse all", and
 * edge arrows that only appear on hover and only on the side there is more to
 * see.
 *
 * The cards are passed in rather than derived from a list of items, because the
 * shelves differ in what a card is — a poster, a wide progress card, a ranked
 * pair of numeral and poster — while the scrolling is identical in all of them.
 */
export function MediaRail({
  title,
  viewAll,
  children,
  /** Reserves room to the left of the first card, for the Top 10 numerals. */
  className,
  itemCount,
  /** Identity of the shelf's first card. A change resets the shelf to it. */
  resetKey,
}: {
  title: string
  viewAll?: string
  children: ReactNode
  className?: string
  /** Re-measures the edges when the shelf's contents change length. */
  itemCount?: number
  /** Identity of the shelf's first card. A change resets the shelf to it. */
  resetKey?: string
}) {
  const rail = useRef<HTMLDivElement>(null)
  const headingId = useId()
  const [edges, setEdges] = useState({ start: true, end: true })

  const measure = useCallback(() => {
    const node = rail.current
    if (!node) return
    setEdges({
      start: node.scrollLeft <= 2,
      end: node.scrollLeft + node.clientWidth >= node.scrollWidth - 2,
    })
  }, [])

  useEffect(() => {
    const node = rail.current
    if (!node) return
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(node)
    return () => observer.disconnect()
  }, [measure, itemCount])

  // A shelf always reads from its first position: at mount, because the browser
  // may otherwise restore a stale horizontal offset across a reload, and when
  // the leading item changes, so a freshly synced title is visibly the new
  // first card while everything after it shifts one slot to the right.
  useLayoutEffect(() => {
    const node = rail.current
    if (node && node.scrollLeft !== 0) node.scrollLeft = 0
  }, [resetKey])

  const move = (direction: -1 | 1) => {
    const node = rail.current
    if (!node) return
    node.scrollBy({ left: direction * node.clientWidth * 0.85, behavior: "smooth" })
  }

  return (
    <section className="group/row flex flex-col gap-3" aria-labelledby={headingId}>
      {/* Marker, title, then a hairline out to the page edge. The rule is what
          makes a shelf read as a band across the page rather than as a row of
          pictures with a caption above it. */}
      <div className="flex items-center gap-3 px-6 sm:px-10 lg:px-14">
        <span className="rail-marker" aria-hidden />
        <h2 id={headingId} className="text-base font-semibold tracking-tight sm:text-lg">
          {title}
        </h2>
        <span className="rail-rule min-w-6 flex-1" aria-hidden />
        {viewAll && (
          <Link
            to={viewAll}
            className="data-label flex shrink-0 items-center gap-1 rounded-sm text-muted-foreground transition-colors hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            All
            <ChevronRight className="size-3.5" />
          </Link>
        )}
      </div>

      <div className="relative">
        <div
          ref={rail}
          role="region"
          aria-labelledby={headingId}
          tabIndex={0}
          onScroll={measure}
          onKeyDown={(event) => {
            if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
              event.preventDefault()
              move(event.key === "ArrowLeft" ? -1 : 1)
            }
          }}
          className={cn(
            "home-media-rail flex snap-x snap-mandatory gap-[var(--card-gap)] overflow-x-auto px-6 pt-1 pb-4 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset sm:px-10 lg:px-14",
            className,
          )}
        >
          {children}
        </div>

        <RailArrow side="start" title={title} hidden={edges.start} onClick={() => move(-1)} />
        <RailArrow side="end" title={title} hidden={edges.end} onClick={() => move(1)} />
      </div>
    </section>
  )
}

function RailArrow({
  side,
  title,
  hidden,
  onClick,
}: {
  side: "start" | "end"
  title: string
  hidden: boolean
  onClick: () => void
}) {
  const Icon = side === "start" ? ChevronLeft : ChevronRight
  return (
    <button
      type="button"
      aria-label={side === "start" ? `Previous ${title}` : `Next ${title}`}
      disabled={hidden}
      onClick={onClick}
      className={cn(
        "absolute top-1/2 z-20 flex h-24 w-8 -translate-y-1/2 items-center justify-center rounded-media border border-white/10 bg-background/85 text-foreground/80 backdrop-blur-sm transition hover:border-primary/60 hover:text-primary focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none",
        side === "start" ? "left-1 sm:left-2" : "right-1 sm:right-2",
        hidden ? "opacity-0" : "opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100",
      )}
    >
      <Icon className="size-7" />
    </button>
  )
}
