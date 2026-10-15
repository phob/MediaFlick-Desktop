import { useVirtualizer } from "@tanstack/react-virtual"
import { LibraryBig } from "lucide-react"
import { useEffect, useImperativeHandle, useLayoutEffect, useMemo, useState, type Ref } from "react"
import { MediaCard } from "@/components/MediaCard"
import { PageEmptyState, PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { useScrollMargin } from "@/hooks/use-scroll-margin"
import { PAGE_SIZE, type ItemSummary } from "@/lib/api"
import type { BrowseSource } from "@/lib/libraries"
import { useBrowsePages } from "@/lib/queries"

const FALLBACK = { poster: 168, gap: 18, card: 306 }

function cardKey(item: ItemSummary) {
  return item.playlistIndex === undefined ? item.id : `${item.id}:${item.playlistIndex}`
}

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
    window.addEventListener("resize", measure)
    return () => { observer.disconnect(); window.removeEventListener("resize", measure) }
  }, [element])

  return metrics
}

export interface ItemGridHandle {
  jumpTo: (index: number) => void
}

export interface ItemGridProps {
  source: BrowseSource
  scroller: HTMLElement | null
  onTotal?: (total: number | null) => void
  empty?: string
  filling?: boolean
  ref?: Ref<ItemGridHandle>
}

/**
 * The library's windowed poster grid. It is sized from the server's `total` on
 * the first page's arrival and fills itself from a sparse map of pages, so the
 * scrollbar never resizes as pages load — scrolling does not snap back the way
 * an append-as-you-go list does.
 */
export function ItemGrid({
  source,
  scroller,
  onTotal,
  empty = "No items match that query.",
  filling = false,
  ref,
}: ItemGridProps) {
  "use no memo"

  const [content, setContent] = useState<HTMLDivElement | null>(null)
  const scrollMargin = useScrollMargin(content, scroller)
  const { columns, rowHeight, cardHeight, gap } = useGridMetrics(content)
  const [visibleRows, setVisibleRows] = useState({ first: 0, last: 0 })
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

  const results = useBrowsePages(source, pages)
  const total = results[0]?.data?.total ?? null
  // A background refetch can fail while every cached card is still valid. Only
  // page zero can make the whole view unavailable; later page failures remain
  // as unloaded slots until the query retries.
  const error = results[0]?.data ? null : results[0]?.error

  const loaded = new Map<number, ItemSummary[]>()
  pages.forEach((page, index) => {
    const data = results[index]?.data
    if (data) loaded.set(page, data.items)
  })

  useEffect(() => {
    onTotal?.(total)
  }, [total, onTotal])

  const rows = total === null ? 0 : Math.ceil(total / columns)
  // TanStack Virtual is intentionally outside compiler memoization above. Its
  // known diagnostic does not honor `use no memo`: TanStack/virtual#1119.
  // oxlint-disable-next-line react/incompatible-library -- upstream false positive after explicit compiler opt-out
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scroller,
    estimateSize: () => rowHeight,
    overscan: 2,
    scrollMargin: scrollMargin ?? 0,
    enabled: scrollMargin !== null,
  })

  useImperativeHandle(ref, () => ({
    jumpTo: (index) => virtualizer.scrollToOffset(Math.floor(index / columns) * rowHeight),
  }))

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
    <div ref={setContent}>
      {error ? (
        <PageErrorState
          title="Could not load these items"
          description={error.message}
          action={<Button variant="outline" onClick={() => void Promise.all(results.map((result) => result.refetch()))}>Try again</Button>}
        />
      ) : total === null || (total === 0 && filling) ? (
        <PlaceholderGrid columns={columns} gap={gap} cardHeight={cardHeight} />
      ) : total === 0 ? (
        <PageEmptyState
          icon={<LibraryBig className="size-6" />}
          title="Nothing to show"
          description={empty}
        />
      ) : (
        <div data-item-grid-canvas className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
          {items.map((row) => (
            <div
              key={row.key}
              className="virtual-card-row absolute top-0 left-0 flex w-full"
              style={{
                height: cardHeight,
                gap,
                transform: `translateY(${row.start - (scrollMargin ?? 0)}px)`,
              }}
            >
              {Array.from({ length: columns }, (_, column) => {
                const index = row.index * columns + column
                if (index >= total) return null
                const item = itemAt(index)
                return item ? (
                  <MediaCard key={cardKey(item)} item={item} className="catalog-card shrink-0" />
                ) : (
                  <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
                )
              })}
            </div>
          ))}
        </div>
      )}
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
