import { useVirtualizer } from "@tanstack/react-virtual"
import { useLayoutEffect, useState, type ReactNode } from "react"

const FALLBACK_POSTER_WIDTH = 168
const FALLBACK_CARD_HEIGHT = 306
const FALLBACK_GAP = 18
const INITIAL_CARDS = 12

interface GridLayout {
  columns: number
  cardHeight: number
  gap: number
  scrollMargin: number | null
}

function cssNumber(element: HTMLElement, name: string, fallback: number) {
  return Number.parseFloat(getComputedStyle(element).getPropertyValue(name)) || fallback
}

function routeScroller() {
  return document.querySelector<HTMLElement>(".content-viewport")
}

/**
 * A fixed-width poster grid that shares the route's scroll container. Only
 * visible rows and a two-row buffer mount, which bounds card hooks and eager
 * image work even when a provider profile contains hundreds of titles.
 */
export function CollectionTitleGrid<T>({
  items,
  itemKey,
  renderItem,
}: {
  items: readonly T[]
  itemKey: (item: T) => string
  renderItem: (item: T) => ReactNode
}) {
  "use no memo"

  const [element, setElement] = useState<HTMLDivElement | null>(null)
  const [scroller] = useState(routeScroller)
  const [layout, setLayout] = useState<GridLayout>({
    columns: 1,
    cardHeight: FALLBACK_CARD_HEIGHT,
    gap: FALLBACK_GAP,
    scrollMargin: null,
  })

  useLayoutEffect(() => {
    if (!element || !scroller) return
    const measure = () => {
      const poster = cssNumber(element, "--poster-width", FALLBACK_POSTER_WIDTH)
      const card = cssNumber(element, "--card-height", FALLBACK_CARD_HEIGHT)
      const gap = cssNumber(element, "--card-gap", FALLBACK_GAP)
      const columns = Math.max(1, Math.floor((element.clientWidth + gap) / (poster + gap)))
      const scrollMargin = element.getBoundingClientRect().top
        - scroller.getBoundingClientRect().top
        + scroller.scrollTop
      setLayout((previous) => {
        if (
          previous.columns === columns
          && previous.cardHeight === card
          && previous.gap === gap
          && previous.scrollMargin === scrollMargin
        ) return previous
        return { columns, cardHeight: card, gap, scrollMargin }
      })
    }

    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    observer.observe(scroller)
    const page = element.closest<HTMLElement>(".detail-page")
    if (page) observer.observe(page)
    return () => observer.disconnect()
  }, [element, scroller])

  const rows = Math.ceil(items.length / layout.columns)
  const ready = scroller !== null && layout.scrollMargin !== null
  // TanStack Virtual is intentionally outside compiler memoization. Its
  // current diagnostics do not recognize the explicit compiler opt-out.
  // oxlint-disable-next-line react/incompatible-library -- upstream false positive after explicit compiler opt-out
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scroller,
    estimateSize: () => layout.cardHeight,
    getItemKey: (row) => `${layout.columns}:${itemKey(items[row * layout.columns]!)}`,
    gap: layout.gap,
    overscan: 2,
    scrollMargin: layout.scrollMargin ?? 0,
    enabled: ready,
  })

  if (!scroller) {
    return (
      <div className="flex flex-wrap gap-[var(--card-gap)]">
        {items.map((item) => <div key={itemKey(item)}>{renderItem(item)}</div>)}
      </div>
    )
  }

  if (!ready) {
    return (
      <div ref={setElement} className="flex flex-wrap gap-[var(--card-gap)]">
        {items.slice(0, INITIAL_CARDS).map((item) => (
          <div key={itemKey(item)}>{renderItem(item)}</div>
        ))}
      </div>
    )
  }

  return (
    <div
      ref={setElement}
      className="relative w-full"
      style={{ height: virtualizer.getTotalSize() }}
    >
      {virtualizer.getVirtualItems().map((row) => {
        const start = row.index * layout.columns
        const rowItems = items.slice(start, start + layout.columns)
        return (
          <div
            key={row.key}
            ref={virtualizer.measureElement}
            data-index={row.index}
            className="absolute top-0 left-0 flex w-full items-start"
            style={{
              gap: layout.gap,
              minHeight: layout.cardHeight,
              transform: `translateY(${row.start - layout.scrollMargin!}px)`,
            }}
          >
            {rowItems.map((item) => <div key={itemKey(item)}>{renderItem(item)}</div>)}
          </div>
        )
      })}
    </div>
  )
}
