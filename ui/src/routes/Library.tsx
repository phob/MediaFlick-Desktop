import { useCallback, useEffect, useState, type ComponentType } from "react"
import { Link, useSearchParams } from "react-router-dom"
import { ItemGrid, type ItemGridProps } from "@/components/ItemGrid"
import { LibraryFilters } from "@/components/LibraryFilters"
import { MediaCard } from "@/components/MediaCard"
import { PageErrorState, PageHeader } from "@/components/PageHeader"
import { CastDiscover, type CastDiscoverProps } from "@/components/seerr/CastDiscover"
import { NotInYourLibrary } from "@/components/seerr/NotInYourLibrary"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import type { PersonIdentity } from "@/lib/api"
import {
  castSearchPath,
  readCastSearch,
  writeResolvedCastPerson,
} from "@/lib/cast-search"
import {
  libraryItemQuery,
  libraryKind,
  readLibraryFilters,
  type LibraryFilterState,
  writeLibraryFilters,
} from "@/lib/library-filters"
import { useItems, usePersonResolution } from "@/lib/queries"

/** How many of a matched person's titles the search page shows inline. */
const SEARCH_PERSON_ROW_LIMIT = 12

export interface LibraryComponents {
  ItemGrid: ComponentType<ItemGridProps>
  CastDiscover: ComponentType<CastDiscoverProps>
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
  // No exact person, still resolving, or the server is unreachable: the
  // local results stand alone, silently.
  if (!person || !titles.data?.items.length) return null

  return (
    <section className="flex flex-col gap-4 pt-8" aria-label={`Titles featuring ${person.name}`}>
      <div className="flex items-baseline justify-between gap-4">
        <h2 className="section-title">Featuring {person.name}</h2>
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
        Jellyfin could not identify an exact cast member named {personName}. No name-only results are shown, so different people are never mixed together.
      </p>
    )
  }
  return (
    <div className="flex flex-col gap-3 py-4">
      <p className="text-sm text-muted-foreground">
        More than one Jellyfin person has this name. Choose the exact cast member:
      </p>
      <div className="flex flex-wrap gap-2" role="list" aria-label="Matching cast members">
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
  const Grid = components?.ItemGrid ?? ItemGrid
  const CastDiscovery = components?.CastDiscover ?? CastDiscover
  const [params, setParams] = useSearchParams()
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

  // Enrich an id from either provider into a durable two-provider deep link.
  // Replace avoids adding a synthetic history stop after the original click.
  useEffect(() => {
    if (!castPerson || !resolved) return
    if (
      castPerson.jellyfinId === resolved.jellyfinId
      && castPerson.tmdbId === resolved.tmdbId
      && castPerson.name === resolved.name
    ) return
    setParams((previous) => writeResolvedCastPerson(previous, resolved), { replace: true })
  }, [castPerson, resolved, setParams])

  const search = params.get("search") ?? ""
  const filters = readLibraryFilters(params)
  const favorite = filters.favorite
  const kind = libraryKind(params)

  // The URL is the filter state, so a filtered view is linkable and survives a
  // reload — the app scheme already serves `index.html` for unknown paths.
  const updateFilters = useCallback(
    (patch: Partial<LibraryFilterState>) => {
      setParams(
        (previous) => writeLibraryFilters(previous, patch),
        // Each committed choice is navigable. Back/forward therefore restores
        // both the chips and the server query instead of skipping filter state.
        { replace: false },
      )
    },
    [setParams],
  )

  const query = castPerson && jellyfinPersonId
    ? { personId: jellyfinPersonId }
    : libraryItemQuery(params)
  const globalFavoritesView = favorite && !params.has("kind")

  const title = castPerson
    ? personName
    : search
      ? `Results for “${search}”`
      : globalFavoritesView
        ? "My List"
        : kind === "Series"
          ? "Series"
          : kind === "Movie"
            ? "Movies"
            : kind === "Movie,Series"
              ? "Movies & Series"
            : "Your library"
  const description = castPerson
    ? `Titles featuring ${personName} on your Jellyfin server, followed by more to discover through Seerr.`
    : search
      ? "Everything in your library that matches, with requestable titles from Seerr below."
      : globalFavoritesView
        ? "The movies and series you saved for later, all in one place."
        : kind === "Series"
          ? "Find your next episode, revisit a favorite, or start something new."
          : kind === "Movie"
            ? "Your movie collection, ready to browse and play."
            : "Your movies and series, ready to browse and play."

  const castDiscover = castPerson ? (
    <CastDiscovery
      personName={personName}
      jellyfinId={jellyfinPersonId}
      tmdbId={tmdbPersonId}
      resolving={resolution.isPending}
      resolutionError={jellyfinPersonId ? resolution.error : null}
    />
  ) : undefined

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        eyebrow={castPerson ? "Cast search" : search ? "Search" : "Your library"}
        title={title}
        description={description}
      />
      {!castPerson && <LibraryFilters value={filters} onChange={updateFilters} total={total} />}
      {castPerson && (
        <h2 className="section-title px-6 pt-4 sm:px-10 lg:px-14">On your Jellyfin server</h2>
      )}
      <div className="min-h-0 flex-1">
        {castPerson && !jellyfinPersonId ? (
          <div className="h-full overflow-y-auto px-6 pt-5 pb-8 sm:px-10 lg:px-14">
            {resolution.isPending ? (
              <div
                className="flex flex-wrap gap-[var(--card-gap)]"
                aria-label="Loading server cast results"
              >
                {Array.from({ length: 4 }, (_, index) => (
                  <Skeleton key={index} className="h-poster-h w-poster-w rounded-lg" />
                ))}
              </div>
            ) : resolution.error ? (
              <PageErrorState
                title="Could not resolve this cast member"
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
          <Grid
            query={query}
            onTotal={setTotal}
            empty={
              castPerson
                ? `No Jellyfin titles feature ${personName}.`
                : search
                  ? `Nothing matches “${search}”.`
                  : "No items match that query."
            }
            // Inside the grid's own scroller: it owns the scroll container and
            // the virtualized height, so the block cannot simply follow it.
            footer={
              castPerson ? (
                castDiscover
              ) : search ? (
                <>
                  <SearchPersonSection term={search} />
                  <NotInYourLibrary term={search} />
                </>
              ) : undefined
            }
          />
        )}
      </div>
    </div>
  )
}
