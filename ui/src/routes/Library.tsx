import { useCallback, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { ItemGrid } from "@/components/ItemGrid"
import { LibraryFilters, type LibraryFilterState } from "@/components/LibraryFilters"

export default function Library() {
  const [params, setParams] = useSearchParams()
  const [total, setTotal] = useState<number | null>(null)

  const search = params.get("search") ?? ""
  const favorite = params.get("favorite") === "true"
  // A search or a favorites view spans the whole library; plain browsing
  // defaults to movies.
  const kind = params.has("kind") ? (params.get("kind") ?? "") : search || favorite ? "" : "Movie"

  const filters: LibraryFilterState = {
    sort: params.get("sort") ?? "name",
    genre: params.get("genre") ?? "",
    watched: params.get("watched") ?? "",
  }

  // The URL is the filter state, so a filtered view is linkable and survives a
  // reload — the app scheme already serves `index.html` for unknown paths.
  const updateFilters = useCallback(
    (patch: Partial<LibraryFilterState>) => {
      setParams(
        (previous) => {
          const next = new URLSearchParams(previous)
          for (const [key, value] of Object.entries(patch)) {
            if (value) next.set(key, value)
            else next.delete(key)
          }
          return next
        },
        { replace: true },
      )
    },
    [setParams],
  )

  const query = {
    search,
    kind,
    favorite: favorite || undefined,
    genre: filters.genre,
    sort: filters.sort,
    watched: filters.watched as "true" | "false" | "",
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <LibraryFilters value={filters} onChange={updateFilters} total={total} />
      <div className="min-h-0 flex-1">
        <ItemGrid
          query={query}
          onTotal={setTotal}
          empty={search ? `Nothing matches “${search}”.` : "No items match that query."}
        />
      </div>
    </div>
  )
}
