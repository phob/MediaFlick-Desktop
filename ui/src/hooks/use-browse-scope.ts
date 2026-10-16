import { useParams, useSearchParams } from "react-router-dom"
import type { BrowseCapabilities, Library } from "@/lib/api"
import type { ListingSource } from "@/lib/libraries"
import { libraryItemQuery, libraryQuery } from "@/lib/library-filters"
import { useLibraries } from "@/lib/queries"

export type BrowseScope =
  | { status: "loading" }
  | { status: "failed"; error: Error; retry: () => void }
  | { status: "missing" }
  | {
      status: "ready"
      title: string
      eyebrow: string
      browse: BrowseCapabilities
      library: Library | null
      source: (params: URLSearchParams) => ListingSource
    }

const KIND_TITLES: Record<string, string> = {
  Movie: "Movies",
  Series: "Series",
  "Movie,Series": "Movies & Series",
}

export function useBrowseScope(): BrowseScope {
  const { libraryId } = useParams<{ libraryId: string }>()
  const [params] = useSearchParams()
  const libraries = useLibraries()
  const data = libraries.data
  const unavailable: BrowseScope = libraries.error
    ? { status: "failed", error: libraries.error, retry: () => void libraries.refetch() }
    : { status: "loading" }
  if (!data) return unavailable

  if (libraryId === undefined) {
    const browse = data.crossLibrary
    const search = params.get("search") ?? ""
    const query = libraryItemQuery(params, browse)
    const title = search
      ? `Results for “${search}”`
      : params.get("favorite") === "true" && !params.has("kind")
        ? "Favorites"
        : KIND_TITLES[query.kind ?? ""] ?? "Library"
    return {
      status: "ready",
      title,
      eyebrow: search ? "Search" : "Library",
      browse,
      library: null,
      source: (current) => ({ scope: "titles", query: libraryItemQuery(current, browse) }),
    }
  }

  const library = data.libraries.find((candidate) => candidate.id === libraryId)
  if (!library) return data.known ? { status: "missing" } : unavailable
  return {
    status: "ready",
    title: library.name,
    eyebrow: "Library",
    browse: library.browse,
    library,
    source: (current) => ({
      scope: "library",
      libraryId: library.id,
      query: libraryQuery(current, library.browse),
    }),
  }
}
