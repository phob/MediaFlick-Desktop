import { useScrollRestoration } from "@/lib/scroll-restoration"
import { toast } from "sonner"
import { api } from "@/lib/api"
import { DEFAULT_VIEWING, useViewing } from "@/lib/viewing"
import { useBrowsing } from "@/lib/queries"
import { useEffect, useRef, useState, type ComponentType } from "react"
import { Link, useSearchParams } from "react-router-dom"
import { ItemGrid, type ItemGridHandle, type ItemGridProps } from "@/components/ItemGrid"
import { LetterJumpRail } from "@/components/LetterJumpRail"
import { LibraryFilters } from "@/components/LibraryFilters"
import { MediaCard } from "@/components/MediaCard"
import { PageErrorState, PageHeader } from "@/components/PageHeader"
import { CastDiscover, type CastDiscoverProps } from "@/components/seerr/CastDiscover"
import { NotInYourLibrary } from "@/components/seerr/NotInYourLibrary"
import { ServerCastExtras, type ServerCastExtrasProps } from "@/components/seerr/ServerCastExtras"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import type { PersonIdentity } from "@/lib/api"
import {
  castSearchPath,
  readCastSearch,
  writeResolvedCastPerson,
} from "@/lib/cast-search"
import { useBrowseScope, type BrowseScope } from "@/hooks/use-browse-scope"
import { browsingKey, libraryPath, type BrowseSource } from "@/lib/libraries"
import {
  activeLibraryFilterCount,
  offersFilter,
  readLibraryFilters,
  REMEMBERED_LIBRARY_PARAMS,
  type LibraryFilterState,
  writeLibraryFilters,
} from "@/lib/library-filters"
import { useBrowseLetters, useItems, usePersonResolution } from "@/lib/queries"
import { cn } from "@/lib/utils"

const SEARCH_PERSON_ROW_LIMIT = 12

interface LibraryComponents {
  ItemGrid: ComponentType<ItemGridProps>
  CastDiscover: ComponentType<CastDiscoverProps>
  ServerCastExtras: ComponentType<ServerCastExtrasProps>
}

interface LibraryProps {
  components?: Partial<LibraryComponents>
}

/**
 * A search term that exactly names one Jellyfin person gets their titles as a
 * live section under the instant local results. Cast is not indexed locally,
 * so this is the only way "tom hanks" finds his films — and it resolves over
 * the same exact-identity path the cast pages use, never a fuzzy name match.
 * The server query is scoped to movies and series, so episode credits cannot
 * crowd actual titles out of the row.
 */
export function SearchPersonSection({ term }: { term: string }) {
  const resolution = usePersonResolution({ name: term }, Boolean(term.trim()))
  const person = resolution.data?.person ?? null
  const titles = useItems(
    { personId: person?.jellyfinId, limit: SEARCH_PERSON_ROW_LIMIT },
    person !== null,
  )
  if (!person || !titles.data?.items.length) return null

  return (
    <section className="flex flex-col gap-4 pt-8" aria-label={`Titles with ${person.name}`}>
      <div className="flex items-baseline justify-between gap-4">
        <h2 className="section-title">With {person.name}</h2>
        <Link
          to={castSearchPath({
            jellyfinId: person.jellyfinId,
            tmdbId: person.tmdbId,
            name: person.name,
          })}
          className="text-sm text-muted-foreground outline-none hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
        >
          See all
        </Link>
      </div>
      <div className="flex flex-wrap gap-[var(--card-gap)]">
        {titles.data.items.map((item) => (
          <MediaCard key={item.id} item={item} className="catalog-card" />
        ))}
      </div>
    </section>
  )
}

function CastCandidates({
  candidates,
  personName,
}: {
  candidates: PersonIdentity[]
  personName: string
}) {
  if (!candidates.length) {
    return (
      <p className="py-4 text-sm text-muted-foreground">
        Jellyfin could not identify an exact person named {personName}. No name-only results are shown, so different people are never mixed together.
      </p>
    )
  }
  return (
    <div className="flex flex-col gap-3 py-4">
      <p className="text-sm text-muted-foreground">
        More than one Jellyfin person has this name. Choose the exact person:
      </p>
      <div className="flex flex-wrap gap-2" role="list" aria-label="Matching people">
        {candidates.map((candidate) => (
          <Link
            key={candidate.jellyfinId}
            role="listitem"
            to={castSearchPath({
              jellyfinId: candidate.jellyfinId,
              tmdbId: candidate.tmdbId,
              name: candidate.name,
            })}
            className="rounded-media border border-border bg-card px-4 py-2 text-sm outline-none hover:border-primary hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
          >
            {candidate.name}
            {candidate.tmdbId
              ? ` · TMDB ${candidate.tmdbId}`
              : ` · Jellyfin ${candidate.jellyfinId.slice(0, 8)}`}
          </Link>
        ))}
      </div>
    </div>
  )
}

export default function Library({ components }: LibraryProps = {}) {
  const scope = useBrowseScope()
  switch (scope.status) {
    case "loading":
      return (
        <div className="flex h-full min-h-0 flex-col" aria-busy="true">
          <div className="px-6 pt-8 pb-5 sm:px-10 lg:px-14">
            <Skeleton className="h-9 w-56 rounded-md" />
          </div>
        </div>
      )
    case "failed":
      return (
        <div className="p-6 sm:p-10 lg:p-14">
          <PageErrorState
            title="Could not load your libraries"
            description={scope.error.message}
            action={<Button variant="outline" onClick={scope.retry}>Try again</Button>}
          />
        </div>
      )
    case "missing":
      return (
        <div className="p-6 sm:p-10 lg:p-14">
          <PageErrorState
            title="Library not available"
            description="This library is no longer on your server, or your account cannot see it."
            action={<Button variant="outline" asChild><Link to="/">Go home</Link></Button>}
          />
        </div>
      )
    case "ready":
      return <BrowsePage key={scope.library?.id ?? "titles"} scope={scope} components={components} />
  }
}

function BrowsePage({
  scope,
  components,
}: {
  scope: Extract<BrowseScope, { status: "ready" }>
  components?: Partial<LibraryComponents>
}) {
  const [castScroller, setCastScroller] = useState<HTMLDivElement | null>(null)
  useScrollRestoration(castScroller, "cast-search")
  const [gridScroller, setGridScroller] = useState<HTMLDivElement | null>(null)
  const grid = useRef<ItemGridHandle>(null)
  const Grid = components?.ItemGrid ?? ItemGrid
  const CastDiscovery = components?.CastDiscover ?? CastDiscover
  const ServerExtras = components?.ServerCastExtras ?? ServerCastExtras
  const [routeParams, setParams] = useSearchParams()
  const viewing = useViewing()
  const history = useBrowsing()
  const preferences = viewing.data ?? DEFAULT_VIEWING
  const { browse, library } = scope
  const rememberKey = library ? browsingKey(library.id) : null
  const explicitFilters = REMEMBERED_LIBRARY_PARAMS.some((key) => routeParams.has(key))
  const params = new URLSearchParams(routeParams)
  if (rememberKey && !explicitFilters) {
    const remembered = preferences.rememberFilters ? history.data?.[rememberKey] : undefined
    if (remembered) {
      const saved = new URLSearchParams(remembered.split("?")[1])
      for (const key of REMEMBERED_LIBRARY_PARAMS) { const value = saved.get(key); if (value !== null) params.set(key, value) }
    } else if (preferences.hideWatched && offersFilter(browse, "watched")) params.set("watched", "unwatched")
  }
  const [total, setTotal] = useState<number | null>(null)

  const castPerson = readCastSearch(params)
  const resolution = usePersonResolution(
    {
      jellyfinId: castPerson?.jellyfinId ?? undefined,
      tmdbId: castPerson?.tmdbId ?? undefined,
      name: castPerson?.name,
    },
    castPerson !== null,
  )
  const resolved = resolution.data?.person ?? null
  const jellyfinPersonId = castPerson?.jellyfinId ?? resolved?.jellyfinId ?? null
  const tmdbPersonId = castPerson?.tmdbId ?? resolved?.tmdbId ?? null
  const personName = resolved?.name ?? castPerson?.name ?? "Cast member"

  useEffect(() => {
    if (!castPerson || !resolved) return
    if (
      castPerson.jellyfinId === resolved.jellyfinId
      && castPerson.tmdbId === resolved.tmdbId
      && castPerson.name === resolved.name
    ) return
    setParams((previous) => writeResolvedCastPerson(previous, resolved), { replace: true })
  }, [castPerson, resolved, setParams])

  const search = library ? "" : params.get("search") ?? ""
  const filters = readLibraryFilters(params, browse)

  const updateFilters = (patch: Partial<LibraryFilterState>) => {
    const next = writeLibraryFilters(params, patch, browse)
    next.set("filters", "true")
    setParams(next, {replace:false})
    if (library && rememberKey && preferences.rememberFilters) {
      void api.saveBrowsing(rememberKey, libraryPath(library.id, next)).then(() => { void history.refetch() }).catch((error: Error) => toast.error(`Could not remember library filters: ${error.message}`))
    }
  }

  const listing = scope.source(params)
  const source: BrowseSource = castPerson && jellyfinPersonId
    ? { scope: "titles", query: { personId: jellyfinPersonId } }
    : listing
  const alphabetical = browse.letterIndex && !castPerson && !search && filters.sort === "name"
  const letters = useBrowseLetters(listing, alphabetical)
  const letterIndex = alphabetical ? letters.data?.letters : undefined
  useScrollRestoration(gridScroller, `grid:${JSON.stringify(source)}`, total !== null)

  const title = castPerson ? personName : scope.title
  const castDiscover = castPerson ? (
    <CastDiscovery
      personName={personName}
      jellyfinId={jellyfinPersonId}
      tmdbId={tmdbPersonId}
      resolving={resolution.isPending}
      resolutionError={jellyfinPersonId ? resolution.error : null}
    />
  ) : undefined
  const rail = letterIndex && total
    ? <LetterJumpRail letters={letterIndex} onJump={(index) => grid.current?.jumpTo(index)} />
    : null

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        eyebrow={castPerson ? "Person" : scope.eyebrow}
        title={title}
      />
      {!castPerson && (
        <LibraryFilters
          value={filters}
          onChange={updateFilters}
          total={total}
          capabilities={browse}
          source={listing}
          tvShowsLibrary={library?.kind === "shows"}
        />
      )}
      {castPerson && (
        <h2 className="section-title px-6 pt-4 sm:px-10 lg:px-14">On your Jellyfin server</h2>
      )}
      <div className="min-h-0 flex-1">
        {castPerson && !jellyfinPersonId ? (
          <div ref={setCastScroller} className="h-full overflow-y-auto px-6 pt-5 pb-8 sm:px-10 lg:px-14">
            {resolution.isPending ? (
              <div
                className="flex flex-wrap gap-[var(--card-gap)]"
                aria-label="Loading server titles for this person"
              >
                {Array.from({ length: 4 }, (_, index) => (
                  <Skeleton key={index} className="h-poster-h w-poster-w rounded-lg" />
                ))}
              </div>
            ) : resolution.error ? (
              <PageErrorState
                title="Could not resolve this person"
                description={resolution.error.message}
                action={<Button variant="outline" onClick={() => void resolution.refetch()}>Try again</Button>}
              />
            ) : (
              <CastCandidates
                candidates={resolution.data?.candidates ?? []}
                personName={personName}
              />
            )}
            {castDiscover}
          </div>
        ) : (
          <div className="flex h-full min-h-0">
            <div
              ref={setGridScroller}
              className={cn(
                "h-full min-w-0 flex-1 overflow-x-hidden overflow-y-auto px-6 pt-5 pb-8 sm:px-10 lg:px-14",
                rail && "pr-2 sm:pr-3 lg:pr-4",
              )}
            >
              <Grid
                ref={grid}
                source={source}
                scroller={gridScroller}
                onTotal={setTotal}
                emptyIsPending={Boolean(library?.membershipPending) && !castPerson && activeLibraryFilterCount(filters) === 0}
                empty={
                  castPerson
                    ? `No Jellyfin titles credit ${personName}.`
                    : search
                      ? `Nothing matches “${search}”.`
                      : "No items match that query."
                }
              />
              {castPerson ? (
                <>
                  <ServerExtras
                    personName={personName}
                    jellyfinId={jellyfinPersonId}
                    tmdbId={tmdbPersonId}
                    resolving={resolution.isPending}
                    resolutionError={jellyfinPersonId ? resolution.error : null}
                  />
                  {castDiscover}
                </>
              ) : search ? (
                <>
                  <SearchPersonSection term={search} />
                  <NotInYourLibrary term={search} />
                </>
              ) : null}
            </div>
            {rail}
          </div>
        )}
      </div>
    </div>
  )
}
