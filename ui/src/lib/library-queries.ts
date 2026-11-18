import {
  queryOptions,
  skipToken,
  useQueries,
  useQuery,
} from "@tanstack/react-query"
import {
  PAGE_SIZE,
  api,
  type ItemQuery,
  type PersonResolveQuery,
} from "./api"
import type { TitleKind } from "./kinds"
import type { BrowseSource, ListingSource } from "./libraries"
import { queryKeys } from "./query-client"
import { useStatus } from "./session-queries"

export function useOfficialRatings(kinds: readonly TitleKind[], enabled = true) {
  return useQuery({
    queryKey: queryKeys.officialRatings(kinds),
    queryFn: () => api.officialRatings(kinds),
    staleTime: 30 * 60_000,
    enabled,
  })
}

export function useBrowseLetters(source: ListingSource, enabled = true) {
  return useQuery({
    queryKey: queryKeys.browseLetters(source),
    queryFn: ({ signal }) =>
      source.scope === "library"
        ? api.libraryLetters(source.libraryId, source.query, signal)
        : api.itemLetters(source.query, signal),
    enabled,
  })
}

/**
 * Both library hooks pass the real `queryFn`. The query keeps the options of
 * the observer that rendered last, so one rendered with `skipToken` would
 * leave the next invalidation without a fetcher.
 */
const librariesQuery = queryOptions({ queryKey: queryKeys.libraries, queryFn: api.libraries })

export function useLibraries() {
  const { data: status } = useStatus()
  return useQuery({ ...librariesQuery, enabled: Boolean(status?.authenticated) })
}

export function useCachedLibraries() {
  return useQuery({ ...librariesQuery, enabled: false }).data?.libraries
}

export function useLibraryOptions(id: string | null) {
  return useQuery({
    queryKey: queryKeys.libraryOptions(id ?? ""),
    queryFn: id === null ? skipToken : () => api.libraryOptions(id),
    staleTime: 30 * 60_000,
  })
}

export function useItems(query: ItemQuery, enabled = true) {
  return useQuery({
    queryKey: queryKeys.items(query),
    queryFn: ({ signal }) => api.items(query, signal),
    enabled,
  })
}

export function usePersonResolution(query: PersonResolveQuery, enabled = true) {
  return useQuery({
    queryKey: queryKeys.personResolution(
      query.jellyfinId ?? "",
      query.tmdbId ?? null,
      query.name ?? "",
    ),
    queryFn: ({ signal }) => api.resolvePerson(query, signal),
    enabled,
    retry: false,
  })
}

/**
 * The windowed grid's data source: one query per `PAGE_SIZE` page, so a page
 * already in the cache renders without a round trip and pages scrolled past
 * stay cached under the normal `gcTime`. Callers pass only the pages they can
 * actually see (plus page 0, which anchors `total`).
 */
export function useBrowsePages(source: BrowseSource, pages: number[]) {
  return useQueries({
    queries: pages.map((page) => {
      const offset = page * PAGE_SIZE
      return {
        queryKey: queryKeys.browsePage(source, offset, PAGE_SIZE),
        queryFn: ({ signal }: { signal: AbortSignal }) => {
          switch (source.scope) {
            case "children":
              return api.children(source.itemId, { offset, limit: PAGE_SIZE }, signal)
            case "library":
              return api.libraryItems(source.libraryId, source.query, offset, PAGE_SIZE, signal)
            case "titles":
              return api.items({ ...source.query, limit: PAGE_SIZE, offset }, signal)
          }
        },
      }
    }),
  })
}
