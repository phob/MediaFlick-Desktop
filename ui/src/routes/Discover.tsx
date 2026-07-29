import { Search } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { PageHeader } from "@/components/PageHeader"
import { SeerrResults } from "@/components/seerr/SeerrResults"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { SEERR_DISCOVER_ROWS, type SeerrDiscoverRow } from "@/lib/api"
import { useSeerrDiscover, useSeerrSearch } from "@/lib/queries"

function DiscoverRow({ row, active }: { row: SeerrDiscoverRow; active: boolean }) {
  const results = useSeerrDiscover(row, active)
  const sentinel = useRef<HTMLDivElement>(null)
  const {
    data,
    error,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isPending,
  } = results
  const pages = data?.pages

  // The sentinel sits just below the poster wall. A generous lead means the
  // next page is already arriving before the user reaches the bottom; if the
  // first page is too short to fill the window, it remains visible and pages
  // continue loading until the screen is full.
  useEffect(() => {
    const node = sentinel.current
    if (!node || !active || !hasNextPage || isFetchingNextPage) return

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) void fetchNextPage()
      },
      { rootMargin: "600px 0px" },
    )
    observer.observe(node)
    return () => observer.disconnect()
  }, [active, fetchNextPage, hasNextPage, isFetchingNextPage])

  const items = pages?.flatMap((page) => page.results)

  return (
    <div className="flex flex-col gap-6">
      <SeerrResults
        results={items}
        isPending={isPending}
        error={error}
        empty="Seerr returned nothing for this row."
        placeholders={12}
      />
      {isFetchingNextPage ? (
        <div className="flex flex-wrap gap-[var(--card-gap)]" aria-label="Loading more titles">
          {Array.from({ length: 6 }, (_, index) => (
            <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
          ))}
        </div>
      ) : null}
      {hasNextPage ? <div ref={sentinel} className="h-px" aria-hidden /> : null}
    </div>
  )
}

/**
 * Browsing what the library does not have. The search box here is Seerr's, not
 * the sidebar's: that one is the local cache, and mixing the two would make
 * local results wait on a network round trip.
 */
export default function Discover() {
  const [params, setParams] = useSearchParams()
  const term = params.get("q") ?? ""
  const [draft, setDraft] = useState(term)
  const [row, setRow] = useState<SeerrDiscoverRow>("trending")
  const search = useSeerrSearch(term)

  return (
    <div className="flex min-h-full min-w-0 flex-col">
      <PageHeader
        eyebrow="Beyond your library"
        title="Discover something new"
        description="Explore what’s trending, find a specific title, and request it without leaving MediaFlick."
      />
      <div className="flex min-w-0 flex-col gap-7 px-6 pb-10 sm:px-10 lg:px-14">
        <form
          className="relative max-w-xl"
          onSubmit={(event) => {
            event.preventDefault()
            setParams(draft.trim() ? { q: draft.trim() } : {}, { replace: true })
          }}
        >
          <Search className="pointer-events-none absolute top-1/2 left-4 size-5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Search films and series…"
            aria-label="Search Seerr"
            className="h-12 rounded-xl border-white/10 bg-white/5 pr-4 pl-12 text-base shadow-lg shadow-black/10 placeholder:text-muted-foreground/75"
          />
        </form>

        {term ? (
          <section className="flex flex-col gap-4">
            <h2 className="section-title">
              Results for “{term}”
              {search.data ? (
                <span className="ml-2 text-sm font-normal text-muted-foreground">
                  {search.data.totalResults}
                </span>
              ) : null}
            </h2>
            <SeerrResults
              results={search.data?.results}
              isPending={search.isPending}
              error={search.error}
              empty={`Seerr found nothing for “${term}”.`}
            />
          </section>
        ) : (
          <Tabs
            value={row}
            onValueChange={(value) => setRow(value as SeerrDiscoverRow)}
            className="gap-5"
          >
            <TabsList className="h-11 rounded-xl border border-white/5 bg-white/5 p-1">
              {SEERR_DISCOVER_ROWS.map((entry) => (
                <TabsTrigger
                  key={entry.id}
                  value={entry.id}
                  className="h-full rounded-media px-5 data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                >
                  {entry.label}
                </TabsTrigger>
              ))}
            </TabsList>
            {SEERR_DISCOVER_ROWS.map((entry) => (
              <TabsContent key={entry.id} value={entry.id}>
                <DiscoverRow row={entry.id} active={row === entry.id} />
              </TabsContent>
            ))}
          </Tabs>
        )}
      </div>
    </div>
  )
}
