import { Filter, Search, X } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { PageHeader } from "@/components/PageHeader"
import { SeerrResults } from "@/components/seerr/SeerrResults"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  SEERR_DISCOVER_ROWS,
  seerrImageUrl,
  type SeerrDiscoverFilters,
  type SeerrDiscoverRow,
  type SeerrMediaType,
  type SeerrResult,
} from "@/lib/api"
import {
  DISCOVERY_FILTER_KEYS,
  RELEASE_DECADES,
  defaultDiscoveryFilters,
  discoveryResultSetKey,
  discoveryResultsForSet,
  readDiscoveryFilters,
  type DiscoveryAvailability,
  writeDiscoveryFilters,
} from "@/lib/discovery"
import {
  useCompanion,
  useInfiniteSeerrSearch,
  useSeerrDiscover,
  useSeerrGenres,
} from "@/lib/queries"
import { cn } from "@/lib/utils"

type AvailabilityFilter = DiscoveryAvailability
type SearchMediaFilter = "all" | SeerrMediaType
const BASIC_DISCOVERY_ROWS = SEERR_DISCOVER_ROWS.filter((entry) =>
  ["trending", "movies", "tv"].includes(entry.id),
)

function discoverRow(value: string | null): SeerrDiscoverRow {
  return SEERR_DISCOVER_ROWS.find((row) => row.id === value)?.id ?? "trending"
}

function availabilityFilter(value: string | null): AvailabilityFilter {
  return value === "outside" || value === "library" ? value : "all"
}

function filterResults(
  results: SeerrResult[] | undefined,
  availability: AvailabilityFilter,
  mediaType: SearchMediaFilter = "all",
) {
  return results?.filter((result) => {
    if (mediaType !== "all" && result.mediaType !== mediaType) return false
    if (availability === "outside") return !result.libraryItemId
    if (availability === "library") return Boolean(result.libraryItemId)
    return true
  })
}

/**
 * Shared tail for both catalogue and search pagination. Keeping the observer in
 * a rendered component means filtered-out pages still advance while the
 * sentinel remains visible.
 */
function PaginationTail({
  active = true,
  hasNextPage,
  isFetchingNextPage,
  fetchNextPage,
}: {
  active?: boolean
  hasNextPage: boolean
  isFetchingNextPage: boolean
  fetchNextPage: () => Promise<unknown>
}) {
  const sentinel = useRef<HTMLDivElement>(null)

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

  return (
    <>
      {isFetchingNextPage ? (
        <div className="flex flex-wrap gap-[var(--card-gap)]" aria-label="Loading more titles">
          {Array.from({ length: 6 }, (_, index) => (
            <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-media" />
          ))}
        </div>
      ) : null}
      {hasNextPage ? <div ref={sentinel} className="h-px" aria-hidden /> : null}
    </>
  )
}

function FilterField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className="data-label text-muted-foreground">{label}</span>
      {children}
    </label>
  )
}

function AvailabilitySelect({
  value,
  onChange,
}: {
  value: AvailabilityFilter
  onChange: (value: AvailabilityFilter) => void
}) {
  return (
    <FilterField label="Library">
      <Select value={value} onValueChange={(next) => onChange(next as AvailabilityFilter)}>
        <SelectTrigger className="h-10 min-w-40 border-white/10 bg-white/5">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All titles</SelectItem>
          <SelectItem value="outside">Not in my library</SelectItem>
          <SelectItem value="library">In my library</SelectItem>
        </SelectContent>
      </Select>
    </FilterField>
  )
}

function GenreBrowser({
  mediaType,
  selected,
  onSelect,
}: {
  mediaType: SeerrMediaType
  selected: number | undefined
  onSelect: (genre: number | undefined) => void
}) {
  const genres = useSeerrGenres(mediaType)

  return (
    <section className="flex flex-col gap-3" aria-labelledby="discover-genres">
      <div className="flex items-center gap-3">
        <span className="rail-marker" aria-hidden />
        <h2 id="discover-genres" className="text-base font-semibold tracking-tight">
          Browse by genre
        </h2>
        <span className="rail-rule min-w-6 flex-1" aria-hidden />
      </div>
      {genres.error ? (
        <p className="text-sm text-destructive">{genres.error.message}</p>
      ) : (
        <div className="media-strip -mx-1 flex snap-x gap-3 overflow-x-auto px-1 pt-1 pb-2">
          <button
            type="button"
            aria-pressed={!selected}
            onClick={() => onSelect(undefined)}
            className={cn(
              "group relative h-20 w-36 shrink-0 snap-start overflow-hidden rounded-media border bg-card text-left outline-none transition focus-visible:ring-2 focus-visible:ring-ring",
              !selected ? "border-primary" : "border-white/10 hover:border-white/30",
            )}
          >
            <span className="absolute inset-0 bg-[radial-gradient(circle_at_20%_15%,color-mix(in_srgb,var(--primary)_30%,transparent),transparent_55%),linear-gradient(135deg,var(--card),var(--background))]" />
            <span className="absolute inset-x-3 bottom-3 text-sm font-semibold">All genres</span>
          </button>
          {genres.isPending
            ? Array.from({ length: 7 }, (_, index) => (
                <Skeleton key={index} className="h-20 w-36 shrink-0 rounded-media" />
              ))
            : genres.data?.map((genre) => {
                const backdrop = seerrImageUrl(genre.backdrops[0], "w300")
                return (
                  <button
                    key={genre.id}
                    type="button"
                    aria-pressed={selected === genre.id}
                    onClick={() => onSelect(genre.id)}
                    className={cn(
                      "group relative h-20 w-36 shrink-0 snap-start overflow-hidden rounded-media border bg-card text-left outline-none transition focus-visible:ring-2 focus-visible:ring-ring",
                      selected === genre.id
                        ? "border-primary"
                        : "border-white/10 hover:border-white/30",
                    )}
                  >
                    {backdrop ? (
                      <img
                        src={backdrop}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        className="media-backdrop-image absolute inset-0 h-full w-full object-cover transition duration-200 group-hover:scale-[1.03]"
                      />
                    ) : null}
                    <span className="absolute inset-0 bg-gradient-to-t from-black via-black/45 to-black/10" />
                    <span className="absolute inset-x-3 bottom-3 truncate text-sm font-semibold">
                      {genre.name}
                    </span>
                  </button>
                )
              })}
        </div>
      )}
    </section>
  )
}

function DiscoveryControls({
  row,
  filters,
  onFiltersChange,
  availability,
  onAvailabilityChange,
  advancedDiscovery = true,
  decadeDiscovery = true,
}: {
  row: SeerrDiscoverRow
  filters: SeerrDiscoverFilters
  onFiltersChange: (filters: SeerrDiscoverFilters) => void
  availability: AvailabilityFilter
  onAvailabilityChange: (value: AvailabilityFilter) => void
  advancedDiscovery?: boolean
  decadeDiscovery?: boolean
}) {
  const catalogue = row === "movies" || row === "tv"
  const catalogueMediaType = row === "movies" ? "movie" : "tv"

  return (
    <div className="flex flex-wrap items-end gap-3 border-y border-white/5 bg-white/[0.025] px-4 py-3">
      <div className="mr-1 flex h-10 items-center gap-2 text-sm font-medium text-foreground/80">
        <Filter className="size-4 text-primary" />
        Refine
      </div>

      {advancedDiscovery && row === "trending" ? (
        <>
          <FilterField label="Format">
            <Select
              value={filters.mediaType ?? "all"}
              onValueChange={(mediaType) =>
                onFiltersChange({
                  ...filters,
                  mediaType: mediaType as NonNullable<SeerrDiscoverFilters["mediaType"]>,
                })
              }
            >
              <SelectTrigger className="h-10 min-w-32 border-white/10 bg-white/5">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">Films & series</SelectItem>
                <SelectItem value="movie">Films</SelectItem>
                <SelectItem value="tv">Series</SelectItem>
              </SelectContent>
            </Select>
          </FilterField>
          <FilterField label="Window">
            <Select
              value={filters.timeWindow ?? "day"}
              onValueChange={(timeWindow) =>
                onFiltersChange({
                  ...filters,
                  timeWindow: timeWindow as NonNullable<SeerrDiscoverFilters["timeWindow"]>,
                })
              }
            >
              <SelectTrigger className="h-10 min-w-28 border-white/10 bg-white/5">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="day">Today</SelectItem>
                <SelectItem value="week">This week</SelectItem>
              </SelectContent>
            </Select>
          </FilterField>
        </>
      ) : null}

      {advancedDiscovery && catalogue ? (
        <>
          <FilterField label="Sort">
            <Select
              value={filters.sort ?? "popular"}
              onValueChange={(sort) =>
                onFiltersChange({
                  ...filters,
                  sort: sort as NonNullable<SeerrDiscoverFilters["sort"]>,
                })
              }
            >
              <SelectTrigger className="h-10 min-w-36 border-white/10 bg-white/5">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="popular">Most popular</SelectItem>
                <SelectItem value="rating">Highest rated</SelectItem>
                <SelectItem value="newest">Newest first</SelectItem>
              </SelectContent>
            </Select>
          </FilterField>
          {decadeDiscovery ? (
            <FilterField label="Release decade">
              <Select
                value={filters.decade ? String(filters.decade) : "all"}
                onValueChange={(decade) =>
                  onFiltersChange({
                    ...filters,
                    decade: decade === "all" ? undefined : Number(decade),
                  })
                }
              >
                <SelectTrigger
                  aria-label="Filter by release decade"
                  className="h-10 min-w-52 border-white/10 bg-white/5"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">Any decade</SelectItem>
                  {RELEASE_DECADES[catalogueMediaType].map((decade) => (
                    <SelectItem key={decade.value} value={String(decade.value)}>
                      {decade.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FilterField>
          ) : null}
          <FilterField label="Minimum score">
            <Select
              value={String(filters.minRating ?? 0)}
              onValueChange={(rating) =>
                onFiltersChange({
                  ...filters,
                  minRating: Number(rating) || undefined,
                })
              }
            >
              <SelectTrigger className="h-10 min-w-32 border-white/10 bg-white/5">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="0">Any score</SelectItem>
                <SelectItem value="6">60% or better</SelectItem>
                <SelectItem value="7">70% or better</SelectItem>
                <SelectItem value="8">80% or better</SelectItem>
              </SelectContent>
            </Select>
          </FilterField>
        </>
      ) : null}

      <AvailabilitySelect value={availability} onChange={onAvailabilityChange} />
    </div>
  )
}

function DiscoverRow({
  row,
  filters,
  availability,
  resultSetKey,
}: {
  row: SeerrDiscoverRow
  filters: SeerrDiscoverFilters
  availability: AvailabilityFilter
  resultSetKey: string
}) {
  const results = useSeerrDiscover(row, filters, availability)
  const pages = results.data?.pages
  const items = discoveryResultsForSet(resultSetKey, pages)
  const visible = filterResults(items, availability)
  const metadata = SEERR_DISCOVER_ROWS.find((entry) => entry.id === row)!
  const total = pages?.find((page) => page.resultSetKey === resultSetKey)?.totalResults

  return (
    <section className="flex flex-col gap-5" aria-labelledby="discover-results">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <h2 id="discover-results" className="section-title">
            {metadata.title}
          </h2>
          <p className="text-sm text-muted-foreground">{metadata.description}</p>
        </div>
        {typeof total === "number" ? (
          <span className="data-label text-muted-foreground">
            {visible?.length ?? 0} loaded / {total.toLocaleString()}
          </span>
        ) : null}
      </div>
      <SeerrResults
        results={visible}
        isPending={results.isPending}
        error={results.error}
        empty="No titles in this feed match the current discovery filters."
        placeholders={12}
        resultSetKey={resultSetKey}
      />
      <PaginationTail
        hasNextPage={Boolean(results.hasNextPage)}
        isFetchingNextPage={results.isFetchingNextPage}
        fetchNextPage={results.fetchNextPage}
      />
    </section>
  )
}

function SearchResults({
  term,
  availability,
  onAvailabilityChange,
}: {
  term: string
  availability: AvailabilityFilter
  onAvailabilityChange: (value: AvailabilityFilter) => void
}) {
  const [mediaType, setMediaType] = useState<SearchMediaFilter>("all")
  const search = useInfiniteSeerrSearch(term)
  const pages = search.data?.pages
  const items = pages?.flatMap((page) => page.results)
  const visible = filterResults(items, availability, mediaType)
  const total = pages?.[0]?.totalResults

  return (
    <section className="flex flex-col gap-5" aria-labelledby="search-results">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <h2 id="search-results" className="section-title">
            Results for “{term}”
          </h2>
          <p className="text-sm text-muted-foreground">
            Search keeps loading as you scroll, just like the discovery feeds.
          </p>
        </div>
        {typeof total === "number" ? (
          <span className="data-label text-muted-foreground">
            {visible?.length ?? 0} loaded / {total.toLocaleString()}
          </span>
        ) : null}
      </div>
      <div className="flex flex-wrap items-end gap-3 border-y border-white/5 bg-white/[0.025] px-4 py-3">
        <div className="mr-1 flex h-10 items-center gap-2 text-sm font-medium text-foreground/80">
          <Filter className="size-4 text-primary" />
          Refine
        </div>
        <FilterField label="Format">
          <Select value={mediaType} onValueChange={(value) => setMediaType(value as SearchMediaFilter)}>
            <SelectTrigger className="h-10 min-w-32 border-white/10 bg-white/5">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">Films & series</SelectItem>
              <SelectItem value="movie">Films</SelectItem>
              <SelectItem value="tv">Series</SelectItem>
            </SelectContent>
          </Select>
        </FilterField>
        <AvailabilitySelect value={availability} onChange={onAvailabilityChange} />
      </div>
      <SeerrResults
        results={visible}
        isPending={search.isPending}
        error={search.error}
        empty={`No Seerr results for “${term}” match these filters.`}
        placeholders={12}
      />
      <PaginationTail
        hasNextPage={Boolean(search.hasNextPage)}
        isFetchingNextPage={search.isFetchingNextPage}
        fetchNextPage={search.fetchNextPage}
      />
    </section>
  )
}

/**
 * Browsing what the library does not have. The search box here is Seerr's, not
 * the sidebar's: that one is the local cache, and mixing the two would make
 * local results wait on a network round trip.
 */
export default function Discover() {
  const [params, setParams] = useSearchParams()
  const companion = useCompanion()
  const term = params.get("q") ?? ""
  const [draft, setDraft] = useState(term)
  const companionCapabilities = companion.data?.info?.capabilities ?? []
  const companionManaged =
    companion.data?.compatible && companionCapabilities.includes("seerr")
  const companionDiscoveryV3 = companionCapabilities.includes("seerr-discovery-v3")
  const companionDiscoveryV4 = companionCapabilities.includes("seerr-discovery-v4")
  const advancedDiscovery =
    !companion.isPending &&
    (!companionManaged ||
      companionCapabilities.includes("seerr-discovery-v2") ||
      companionDiscoveryV3 ||
      companionDiscoveryV4)
  // v3 carried the superseded century parameter. Only v4 understands decade;
  // older Companions still keep their otherwise-supported discovery controls.
  const decadeDiscovery = advancedDiscovery && (!companionManaged || companionDiscoveryV4)
  const discoveryRows = advancedDiscovery
    ? SEERR_DISCOVER_ROWS
    : BASIC_DISCOVERY_ROWS
  const requestedRowValue = params.get("row")
  const requestedRow = discoverRow(requestedRowValue)
  const row = discoveryRows.some((entry) => entry.id === requestedRow)
    ? requestedRow
    : "trending"
  const filters = readDiscoveryFilters(params, row, advancedDiscovery, decadeDiscovery)
  const availability = availabilityFilter(params.get("library"))
  const resultSetKey = discoveryResultSetKey(row, filters, availability)

  useEffect(() => setDraft(term), [term])

  // Capability changes can make a deep-linked row or control unavailable.
  // Canonicalize only after the probe settles; clearing during the pending
  // frame would destroy valid persisted state before the plugin answered.
  useEffect(() => {
    if (companion.isPending) return
    const unsupportedRow =
      requestedRowValue !== null &&
      !discoveryRows.some((entry) => entry.id === requestedRowValue)
    const hasAdvancedFilters = DISCOVERY_FILTER_KEYS.some((key) => params.has(key))
    const clearAdvancedFilters = !advancedDiscovery && hasAdvancedFilters
    const clearUnsupportedDecade = !decadeDiscovery && params.has("decade")
    const clearLegacyCentury = params.has("century")
    if (
      !unsupportedRow &&
      !clearAdvancedFilters &&
      !clearUnsupportedDecade &&
      !clearLegacyCentury
    ) {
      return
    }

    setParams(
      (previous) => {
        const next = new URLSearchParams(previous)
        if (unsupportedRow) next.delete("row")
        if (unsupportedRow || clearAdvancedFilters) {
          for (const key of DISCOVERY_FILTER_KEYS) next.delete(key)
        } else if (clearUnsupportedDecade) {
          next.delete("decade")
        }
        next.delete("century")
        return next
      },
      { replace: true },
    )
  }, [
    advancedDiscovery,
    decadeDiscovery,
    companion.isPending,
    discoveryRows,
    params,
    requestedRowValue,
    setParams,
  ])

  const updateFilters = useCallback(
    (next: SeerrDiscoverFilters) => {
      setParams((previous) => writeDiscoveryFilters(previous, row, next), { replace: true })
    },
    [row, setParams],
  )

  const updateAvailability = useCallback(
    (nextAvailability: AvailabilityFilter) => {
      setParams(
        (previous) => {
          const next = new URLSearchParams(previous)
          if (nextAvailability === "all") next.delete("library")
          else next.set("library", nextAvailability)
          return next
        },
        { replace: true },
      )
    },
    [setParams],
  )

  const submitSearch = () => {
    const next = draft.trim()
    setParams(
      (previous) => {
        const nextParams = new URLSearchParams(previous)
        if (next.length > 1) nextParams.set("q", next)
        else nextParams.delete("q")
        return nextParams
      },
      { replace: true },
    )
  }

  const catalogueMediaType: SeerrMediaType | null =
    row === "movies" ? "movie" : row === "tv" ? "tv" : null

  return (
    <div className="flex min-h-full min-w-0 flex-col">
      <PageHeader
        eyebrow="Beyond your library"
        title="Find your next obsession"
        description="Explore Seerr’s catalogue by what’s trending, what’s next, and the genres you care about—then request it without leaving MediaFlick."
      />
      <div className="flex min-w-0 flex-col gap-7 px-6 pb-10 sm:px-10 lg:px-14">
        <form
          className="relative max-w-2xl"
          onSubmit={(event) => {
            event.preventDefault()
            submitSearch()
          }}
        >
          <Search className="pointer-events-none absolute top-1/2 left-4 size-5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Search films and series…"
            aria-label="Search Seerr"
            minLength={2}
            className="h-12 rounded-xl border-white/10 bg-white/5 pr-12 pl-12 text-base shadow-lg shadow-black/10 placeholder:text-muted-foreground/75"
          />
          {draft ? (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Clear search"
              onClick={() => {
                setDraft("")
                setParams(
                  (previous) => {
                    const next = new URLSearchParams(previous)
                    next.delete("q")
                    return next
                  },
                  { replace: true },
                )
              }}
              className="absolute top-1/2 right-2 -translate-y-1/2 text-muted-foreground"
            >
              <X />
            </Button>
          ) : null}
        </form>

        {term ? (
          <SearchResults
            term={term}
            availability={availability}
            onAvailabilityChange={updateAvailability}
          />
        ) : (
          <Tabs
            value={row}
            onValueChange={(value) => {
              const next = value as SeerrDiscoverRow
              setParams(
                (previous) => {
                  const nextParams = new URLSearchParams(previous)
                  if (next === "trending") nextParams.delete("row")
                  else nextParams.set("row", next)
                  return writeDiscoveryFilters(
                    nextParams,
                    next,
                    defaultDiscoveryFilters(next, advancedDiscovery),
                  )
                },
                { replace: true },
              )
            }}
            className="gap-6"
          >
            <div className="media-strip max-w-full overflow-x-auto pb-1">
              <TabsList className="h-11 rounded-xl border border-white/5 bg-white/5 p-1">
                {discoveryRows.map((entry) => (
                  <TabsTrigger
                    key={entry.id}
                    value={entry.id}
                    className="h-full rounded-media px-5 data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                  >
                    {entry.label}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>

            <TabsContent value={row} className="flex flex-col gap-6">
              {advancedDiscovery && catalogueMediaType ? (
                <GenreBrowser
                  mediaType={catalogueMediaType}
                  selected={filters.genre}
                  onSelect={(genre) => updateFilters({ ...filters, genre })}
                />
              ) : null}
              <DiscoveryControls
                row={row}
                filters={filters}
                onFiltersChange={updateFilters}
                availability={availability}
                onAvailabilityChange={updateAvailability}
                advancedDiscovery={advancedDiscovery}
                decadeDiscovery={decadeDiscovery}
              />
              <DiscoverRow
                key={resultSetKey}
                row={row}
                filters={filters}
                availability={availability}
                resultSetKey={resultSetKey}
              />
            </TabsContent>
          </Tabs>
        )}
      </div>
    </div>
  )
}
