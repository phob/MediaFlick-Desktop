import { useVirtualizer } from "@tanstack/react-virtual"
import { useEffect, useLayoutEffect, useMemo, useState } from "react"
import { MediaCard } from "@/components/MediaCard"
import { Skeleton } from "@/components/ui/skeleton"
import { PAGE_SIZE, type ItemQuery, type ItemSummary } from "@/lib/api"
import { useItemPages } from "@/lib/queries"

/** Fallbacks for the geometry tokens in `app.css`, used before first measure. */
const FALLBACK = { poster: 168, gap: 18, card: 306 }

function readGeometry(element: HTMLElement) {
  const styles = getComputedStyle(element)
  const read = (name: string, fallback: number) =>
    Number.parseFloat(styles.getPropertyValue(name)) || fallback
  return {
    poster: read("--poster-width", FALLBACK.poster),
    gap: read("--card-gap", FALLBACK.gap),
    card: read("--card-height", FALLBACK.card),
  }
}

/**
 * Column count and row height, re-measured on resize. Both come from the CSS
 * tokens rather than hardcoded numbers so the grid follows `--poster-width`.
 */
function useGridMetrics(element: HTMLElement | null) {
  const [metrics, setMetrics] = useState({
    columns: 1,
    rowHeight: FALLBACK.card + FALLBACK.gap,
    cardHeight: FALLBACK.card,
    gap: FALLBACK.gap,
  })

  useLayoutEffect(() => {
    if (!element) return
    const measure = () => {
      const { poster, gap, card } = readGeometry(element)
      const width = element.clientWidth
      const columns = Math.max(1, Math.floor((width + gap) / (poster + gap)))
      setMetrics((previous) =>
        previous.columns === columns && previous.cardHeight === card && previous.gap === gap
          ? previous
          : { columns, rowHeight: card + gap, cardHeight: card, gap },
      )
    }

    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [element])

  return metrics
}

interface ItemGridProps {
  /** Without `limit`/`offset` — the grid owns paging. */
  query: ItemQuery
  /** Reports the result count so the caller can label the view. */
  onTotal?: (total: number | null) => void
  empty?: string
}

/**
 * The library's windowed poster grid. It is sized from the server's `total` on
 * the first page's arrival and fills itself from a sparse map of pages, so the
 * scrollbar never resizes as pages load — scrolling does not snap back the way
 * an append-as-you-go list does.
 */
export function ItemGrid({ query, onTotal, empty = "No items match that query." }: ItemGridProps) {
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  // Measured on the content box, not the scroller: the scroller carries the
  // horizontal padding, which would otherwise be counted as usable width.
  const [content, setContent] = useState<HTMLDivElement | null>(null)
  const { columns, rowHeight, cardHeight, gap } = useGridMetrics(content)
  const [visibleRows, setVisibleRows] = useState({ first: 0, last: 0 })

  // A changed filter is a different list; the old scroll offset means nothing
  // in it, and keeping it would land the user mid-way through the new results.
  const queryKey = JSON.stringify(query)
  useEffect(() => {
    scroller?.scrollTo({ top: 0 })
    setVisibleRows({ first: 0, last: 0 })
  }, [queryKey, scroller])

  // Page 0 is always requested: it is what `total` is read from, so keeping it
  // resident stops the grid collapsing to zero rows when the user scrolls into
  // a region that has not been fetched yet.
  //
  // The bounds are derived in render but the array is memoised on them, not on
  // `visibleRows`: the visible range moves every row, while the page window
  // only moves every `PAGE_SIZE / columns` rows. Keying on the bounds keeps the
  // array identity — and with it the `useQueries` subscriptions — still while
  // the user scrolls within one page.
  const firstPage = Math.floor((visibleRows.first * columns) / PAGE_SIZE)
  const lastPage = Math.floor(((visibleRows.last + 1) * columns - 1) / PAGE_SIZE)
  const pages = useMemo(() => {
    const wanted = new Set([0])
    for (let page = firstPage; page <= lastPage; page += 1) wanted.add(page)
    return [...wanted].sort((a, b) => a - b)
  }, [firstPage, lastPage])

  const results = useItemPages(query, pages)
  const total = results[0]?.data?.total ?? null
  const error = results.find((result) => result.error)?.error

  // Not memoised: `useQueries` hands back a fresh array every render, so any
  // dependency list including it would miss on every pass anyway. Building a
  // map of two or three pages is cheaper than pretending otherwise.
  const loaded = new Map<number, ItemSummary[]>()
  pages.forEach((page, index) => {
    const data = results[index]?.data
    if (data) loaded.set(page, data.items)
  })

  useEffect(() => {
    onTotal?.(total)
  }, [total, onTotal])

  const rows = total === null ? 0 : Math.ceil(total / columns)
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scroller,
    estimateSize: () => rowHeight,
    overscan: 2,
  })

  // Feeding the range back through state is what drives page selection; doing
  // it in render would fetch for a range the virtualizer has not committed.
  const items = virtualizer.getVirtualItems()
  const first = items[0]?.index ?? 0
  const last = items[items.length - 1]?.index ?? 0
  useEffect(() => {
    if (!items.length) return
    setVisibleRows((previous) =>
      previous.first === first && previous.last === last ? previous : { first, last },
    )
  }, [first, last, items.length])

  const itemAt = (index: number) => loaded.get(Math.floor(index / PAGE_SIZE))?.[index % PAGE_SIZE]

  return (
    <div ref={setScroller} className="h-full overflow-y-auto px-6 pb-6">
      <div ref={setContent}>
        {error ? (
          <p className="py-4 text-sm text-destructive">{error.message}</p>
        ) : total === null ? (
          <PlaceholderGrid columns={columns} gap={gap} cardHeight={cardHeight} />
        ) : total === 0 ? (
          <p className="py-4 text-sm text-muted-foreground">{empty}</p>
        ) : (
          <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
            {items.map((row) => (
              <div
                key={row.key}
                className="absolute top-0 left-0 flex w-full"
                style={{
                  height: cardHeight,
                  gap,
                  transform: `translateY(${row.start}px)`,
                }}
              >
                {Array.from({ length: columns }, (_, column) => {
                  const index = row.index * columns + column
                  if (index >= total) return null
                  const item = itemAt(index)
                  return item ? (
                    <MediaCard key={item.id} item={item} className="shrink-0" />
                  ) : (
                    // An unloaded slot keeps its space rather than reflowing
                    // the row once its page lands.
                    <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
                  )
                })}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function PlaceholderGrid({
  columns,
  gap,
  cardHeight,
}: {
  columns: number
  gap: number
  cardHeight: number
}) {
  const rows = 3
  return (
    <div className="flex flex-col" style={{ gap }}>
      {Array.from({ length: rows }, (_, row) => (
        <div key={row} className="flex" style={{ gap, height: cardHeight }}>
          {Array.from({ length: columns }, (_, column) => (
            <Skeleton key={column} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
          ))}
        </div>
      ))}
    </div>
  )
}
