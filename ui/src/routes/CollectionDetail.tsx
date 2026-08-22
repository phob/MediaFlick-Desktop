import { ArrowLeft, Layers } from "lucide-react"
import { Link, useParams, useSearchParams } from "react-router-dom"
import { MediaCard } from "@/components/MediaCard"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { SeerrResults } from "@/components/seerr/SeerrResults"
import {
  COLLECTION_SORTS,
  compareCollectionEntries,
  readCollectionFilters,
  writeCollectionFilters,
  type CollectionFilterState,
} from "@/lib/collection-filters"
import type { ItemSummary } from "@/lib/api"
import { imageUrl, seerrImageUrl } from "@/lib/api"
import { useBoxSet, useCollectionDetail } from "@/lib/queries"
import { useTouchInput } from "@/hooks/use-touch-input"
import { cn } from "@/lib/utils"

const ANY_WATCHED = "__any__"

const WATCHED_OPTIONS = [
  { id: ANY_WATCHED, label: "Any watch status" },
  { id: "false", label: "Unwatched" },
  { id: "true", label: "Watched" },
] as const

/**
 * One collection. Native BoxSets render their movie children as ordinary
 * local cards; where the set carries a TMDB identity, the parts Seerr knows
 * but the library lacks follow underneath with the usual request flow. Sort
 * and watched controls live in the URL, like every other catalog view.
 */
export default function CollectionDetail() {
  const { id } = useParams<{ id: string }>()
  const [params, setParams] = useSearchParams()
  const filters = readCollectionFilters(params)
  const updateFilters = (patch: Partial<CollectionFilterState>) => {
    setParams((previous) => writeCollectionFilters(previous, patch), { replace: true })
  }

  const rawId = id ?? ""
  // A numeric id is a derived TMDB summary (mirroring off or plugin too old);
  // anything else is a Jellyfin BoxSet id.
  const tmdbId = Number(rawId)
  if (Number.isSafeInteger(tmdbId) && tmdbId > 0) {
    return <TmdbCollectionDetail tmdbId={tmdbId} filters={filters} onFilters={updateFilters} />
  }
  if (rawId.length > 0) {
    return <NativeCollectionDetail boxsetId={rawId} filters={filters} onFilters={updateFilters} />
  }
  return (
    <div className="p-6 sm:p-10 lg:p-14">
      <PageErrorState
        title="Invalid collection"
        description="That address does not identify a collection."
      />
    </div>
  )
}

/** Sort and watch-state controls, shared by both collection sources. */
function CollectionControls({
  filters,
  onChange,
}: {
  filters: CollectionFilterState
  onChange: (patch: Partial<CollectionFilterState>) => void
}) {
  const touchInput = useTouchInput()
  const controlClassName = cn(
    "border-white/10 bg-white/5 shadow-none hover:bg-white/8",
    touchInput && "min-h-11",
  )
  return (
    <div className="flex flex-wrap items-center gap-2 px-6 sm:px-10 lg:px-14">
      <Select value={filters.sort} onValueChange={(value) => {
        const option = COLLECTION_SORTS.find((candidate) => candidate.id === value)
        if (option) onChange({ sort: option.id })
      }}>
        <SelectTrigger size={touchInput ? "default" : "sm"} aria-label="Sort by" className={controlClassName}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {COLLECTION_SORTS.map((sort) => (
            <SelectItem key={sort.id} value={sort.id}>
              Sort: {sort.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select
        value={filters.watched || ANY_WATCHED}
        onValueChange={(watched) => onChange({ watched: watched === ANY_WATCHED ? "" : watched })}
      >
        <SelectTrigger
          size={touchInput ? "default" : "sm"}
          aria-label="Watch status"
          className={controlClassName}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {WATCHED_OPTIONS.map((option) => (
            <SelectItem key={option.id} value={option.id}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

/** Applies the URL-driven sort to one section of entries. */
function applySort<T>(rows: T[], filters: CollectionFilterState, entry: (row: T) => {
  name: string
  year: number | null
  rating: number | null
}): T[] {
  if (filters.sort === "release" && entry === tmdbPartEntry) {
    // TMDB already delivers canonical release order.
    return rows
  }
  return [...rows].sort((a, b) => compareCollectionEntries(filters.sort)(entry(a), entry(b)))
}

const tmdbPartEntry = (part: { title: string; year: number | null; voteAverage: number | null }) => ({
  name: part.title,
  year: part.year,
  rating: part.voteAverage,
})

const nativeItemEntry = (item: ItemSummary) => ({
  name: item.name,
  year: item.year,
  rating: item.communityRating,
})

/**
 * The derived TMDB view: parts arrive in release order from TMDB, already
 * joined to local ownership, split by ownership into two sections.
 */
function TmdbCollectionDetail({
  tmdbId,
  filters,
  onFilters,
}: {
  tmdbId: number
  filters: CollectionFilterState
  onFilters: (patch: Partial<CollectionFilterState>) => void
}) {
  const { data, isPending, error, refetch } = useCollectionDetail(tmdbId)

  if (error && !data) {
    return (
      <div className="p-6 sm:p-10 lg:p-14">
        <PageErrorState
          title="Could not load collection"
          description={error.message}
          action={
            <Button variant="outline" onClick={() => void refetch()}>
              Try again
            </Button>
          }
        />
      </div>
    )
  }

  const ownedParts = (data?.parts.filter((part) => part.libraryItemId) ?? [])
    .filter((part) => filters.watched === "" || String(part.played ?? false) === filters.watched)
  const missingParts =
    data?.parts.filter((part) => !part.libraryItemId) ?? []
  const sortedOwnedParts = applySort(ownedParts, filters, tmdbPartEntry)
  const sortedMissingParts = applySort(missingParts, filters, tmdbPartEntry)
  const missing = missingParts.length
  const backdrop = seerrImageUrl(data?.backdropPath, "w1280")
  const poster = seerrImageUrl(data?.posterPath, "w342")

  return (
    <CollectionShell
      backTo="Collections"
      name={data?.name ?? "…"}
      countLine={
        data?.parts.length
          ? `${ownedParts.length} of ${data.parts.length} in your library${missing > 0 ? ` · ${missing} missing` : ""}`
          : null
      }
      overview={data?.overview ?? null}
      backdrop={backdrop}
      poster={poster}
      controls={<CollectionControls filters={filters} onChange={onFilters} />}
    >
      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
        <h2 className="section-title">In your library</h2>
        <SeerrResults
          results={sortedOwnedParts}
          isPending={isPending}
          error={error}
          empty={
            filters.watched === ""
              ? "None of this collection's movies are in your library yet."
              : "No movies in this collection match the watch filter."
          }
          ownedAsLocal
        />
      </section>
      {data && (
        <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
          <h2 className="section-title">Missing from your library</h2>
          <SeerrResults
            results={sortedMissingParts}
            empty="Your library is complete for this collection."
            ownedAsLocal
          />
        </section>
      )}
    </CollectionShell>
  )
}

/** The native BoxSet view: server-owned children plus Seerr-known missing parts. */
function NativeCollectionDetail({
  boxsetId,
  filters,
  onFilters,
}: {
  boxsetId: string
  filters: CollectionFilterState
  onFilters: (patch: Partial<CollectionFilterState>) => void
}) {
  const { data, isPending, error, refetch } = useBoxSet(boxsetId)
  // Where the BoxSet carries a TMDB identity, the derived detail supplies the
  // full part list; everything without a library match becomes a request card.
  const { data: tmdbDetail } = useCollectionDetail(data?.tmdbId ?? null)

  if (error && !data) {
    return (
      <div className="p-6 sm:p-10 lg:p-14">
        <PageErrorState
          title="Could not load collection"
          description={error.message}
          action={
            <Button variant="outline" onClick={() => void refetch()}>
              Try again
            </Button>
          }
        />
      </div>
    )
  }

  const items = (data?.items ?? []).filter(
    (item) => filters.watched === "" || String(item.played) === filters.watched,
  )
  const sortedItems = applySort(items, filters, nativeItemEntry)
  // Ownership comes from the derived join itself: a part the local cache
  // already matches carries a libraryItemId and never shows as missing.
  const missingParts = tmdbDetail?.parts.filter((part) => !part.libraryItemId) ?? []

  return (
    <CollectionShell
      backTo="Collections"
      name={data?.name ?? "…"}
      countLine={
        data
          ? tmdbDetail
            ? `${items.length} of ${tmdbDetail.parts.length} in your library${missingParts.length > 0 ? ` · ${missingParts.length} missing` : ""}`
            : `${items.length} ${items.length === 1 ? "movie" : "movies"}`
          : null
      }
      overview={null}
      backdrop={
        data?.backdropImageTag
          ? imageUrl({ id: data.id, primaryImageTag: null }, "Backdrop", 1280, data.backdropImageTag)
          : null
      }
      poster={data?.primaryImageTag ? imageUrl({ id: data.id, primaryImageTag: data.primaryImageTag }, "Primary", 342) : null}
      controls={<CollectionControls filters={filters} onChange={onFilters} />}
    >
      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
        <h2 className="section-title">In your library</h2>
        {isPending ? (
          <div className="flex flex-wrap gap-[var(--card-gap)]">
            {Array.from({ length: 4 }, (_, index) => (
              <div key={index} className="h-poster-h w-poster-w animate-pulse rounded-lg bg-card" />
            ))}
          </div>
        ) : sortedItems.length ? (
          <div className="flex flex-wrap gap-[var(--card-gap)]">
            {sortedItems.map((item) => (
              <MediaCard key={item.id} item={item} className="catalog-card" />
            ))}
          </div>
        ) : (
          <p className="py-4 text-sm text-muted-foreground">
            {filters.watched === ""
              ? "This collection has no movies yet."
              : "No movies in this collection match the watch filter."}
          </p>
        )}
      </section>
      {tmdbDetail && (
        <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
          <h2 className="section-title">Missing from your library</h2>
          <SeerrResults
            results={missingParts}
            empty="Your library is complete for this collection."
            ownedAsLocal
          />
        </section>
      )}
    </CollectionShell>
  )
}

/** Shared header, controls row, and page frame for both collection sources. */
function CollectionShell({
  backTo,
  name,
  countLine,
  overview,
  backdrop,
  poster,
  controls,
  children,
}: {
  backTo: string
  name: string
  countLine: string | null
  overview: string | null
  backdrop: string | null
  poster: string | null
  controls: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-8 pb-16">
      <header className="relative overflow-hidden">
        {backdrop && (
          <div className="pointer-events-none absolute inset-0" aria-hidden>
            <img
              src={backdrop}
              alt=""
              decoding="async"
              className="media-backdrop-image h-full w-full object-cover opacity-25"
            />
            <div className="absolute inset-0 bg-linear-to-t from-background via-background/70 to-background/30" />
          </div>
        )}
        <div className="relative z-10 flex items-end gap-6 px-6 pt-10 sm:px-10 lg:px-14">
          <div className="hidden h-40 w-27 shrink-0 overflow-hidden rounded-lg bg-card shadow-xl ring-1 ring-white/10 sm:block">
            {poster ? (
              <img src={poster} alt="" decoding="async" className="media-artwork-image h-full w-full object-cover" />
            ) : (
              <div className="flex h-full w-full items-center justify-center text-muted-foreground">
                <Layers className="size-7" aria-hidden />
              </div>
            )}
          </div>
          <div className="min-w-0 flex-1 pb-1">
            <Link
              to="/collections"
              className="inline-flex items-center gap-1 text-sm text-muted-foreground transition hover:text-foreground"
            >
              <ArrowLeft className="size-3.5" />
              {backTo}
            </Link>
            <h1 className="mt-1 truncate text-2xl font-semibold tracking-tight">{name}</h1>
            {countLine && <p className="data-value mt-1 text-muted-foreground">{countLine}</p>}
          </div>
        </div>
        {overview && (
          <p className="relative z-10 mt-4 max-w-3xl px-6 text-sm leading-relaxed text-foreground/85 sm:px-10 lg:px-14">
            {overview}
          </p>
        )}
      </header>
      {controls}
      {children}
    </div>
  )
}
