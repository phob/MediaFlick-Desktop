import { useCallback, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { ItemGrid } from "@/components/ItemGrid"
import { LibraryFilters } from "@/components/LibraryFilters"
import { PageHeader } from "@/components/PageHeader"
import { NotInYourLibrary } from "@/components/seerr/NotInYourLibrary"
import {
  libraryItemQuery,
  libraryKind,
  readLibraryFilters,
  type LibraryFilterState,
  writeLibraryFilters,
} from "@/lib/library-filters"

export default function Library() {
  const [params, setParams] = useSearchParams()
  const [total, setTotal] = useState<number | null>(null)

  const search = params.get("search") ?? ""
  const filters = readLibraryFilters(params)
  const favorite = filters.favorite
  const kind = libraryKind(params)

  // The URL is the filter state, so a filtered view is linkable and survives a
  // reload — the app scheme already serves `index.html` for unknown paths.
  const updateFilters = useCallback(
    (patch: Partial<LibraryFilterState>) => {
      setParams(
        (previous) => {
          return writeLibraryFilters(previous, patch)
        },
        // Each committed choice is navigable. Back/forward therefore restores
        // both the chips and the server query instead of skipping filter state.
        { replace: false },
      )
    },
    [setParams],
  )

  const query = libraryItemQuery(params)

  const globalFavoritesView = favorite && !params.has("kind")

  const title = search
    ? `Results for “${search}”`
    : globalFavoritesView
      ? "My List"
      : kind === "Series"
        ? "Series"
        : kind === "Movie"
          ? "Movies"
          : "Your library"
  const description = search
    ? "Everything in your library that matches, with requestable titles from Seerr below."
    : globalFavoritesView
      ? "The films and shows you saved for later, all in one place."
      : kind === "Series"
        ? "Find your next episode, revisit a favorite, or start something new."
        : "Your film collection, ready to browse and play."

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader eyebrow={search ? "Search" : "Your library"} title={title} description={description} />
      <LibraryFilters value={filters} onChange={updateFilters} total={total} />
      <div className="min-h-0 flex-1">
        <ItemGrid
          query={query}
          onTotal={setTotal}
          empty={search ? `Nothing matches “${search}”.` : "No items match that query."}
          // Inside the grid's own scroller: it owns the scroll container and
          // the virtualized height, so the block cannot simply follow it.
          footer={search ? <NotInYourLibrary term={search} /> : undefined}
        />
      </div>
    </div>
  )
}
