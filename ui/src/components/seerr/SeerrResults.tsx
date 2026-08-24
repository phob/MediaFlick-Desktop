import { SearchX } from "lucide-react"
import { MediaCard } from "@/components/MediaCard"
import { PageEmptyState, PageErrorState } from "@/components/PageHeader"
import { SeerrCard } from "@/components/seerr/SeerrCard"
import { Skeleton } from "@/components/ui/skeleton"
import type { SeerrCapabilities, SeerrResult } from "@/lib/api"
import { discoveryCardKey } from "@/lib/discovery"
import { useItem, useSeerrStatus } from "@/lib/queries"

function ResultCard({
  result,
  capabilities,
  ownedAsLocal,
}: {
  result: SeerrResult
  capabilities?: SeerrCapabilities | null
  ownedAsLocal: boolean
}) {
  const local = useItem(ownedAsLocal ? (result.libraryItemId ?? undefined) : undefined)
  if (ownedAsLocal && result.libraryItemId) {
    if (local.data) {
      return <MediaCard item={local.data} className="catalog-card" />
    }
    if (local.isPending) {
      return <Skeleton className="h-poster-h w-poster-w shrink-0 rounded-lg" />
    }
  }

  return <SeerrCard result={result} capabilities={capabilities} />
}

/**
 * A wrapped row of Seerr results. Not virtualized on purpose: a Seerr page is
 * twenty titles, and the poster wall the library grid windows for is thousands.
 */
export function SeerrResults({
  results,
  isPending,
  error,
  errorTitle = "Could not load discovery results",
  empty = "Nothing found.",
  placeholders = 6,
  resultSetKey,
  ownedAsLocal = false,
}: {
  results: SeerrResult[] | undefined
  isPending?: boolean
  error?: Error | null
  errorTitle?: string
  empty?: string
  placeholders?: number
  resultSetKey?: string
  /** Render matched library items with MediaCard instead of a Seerr card. */
  ownedAsLocal?: boolean
}) {
  const status = useSeerrStatus()

  if (error && !results?.length) {
    return <PageErrorState title={errorTitle} description={error.message} />
  }
  if (isPending) {
    return (
      <div className="flex flex-wrap gap-[var(--card-gap)]">
        {Array.from({ length: placeholders }, (_, index) => (
          <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
        ))}
      </div>
    )
  }
  if (!results?.length) {
    return (
      <PageEmptyState
        icon={<SearchX className="size-6" />}
        title="Nothing found"
        description={empty}
      />
    )
  }

  return (
    <div className="flex flex-wrap gap-[var(--card-gap)]">
      {results.map((result) => (
        <ResultCard
          key={
            resultSetKey
              ? discoveryCardKey(resultSetKey, result)
              : `${result.mediaType}-${result.tmdbId}`
          }
          result={result}
          capabilities={status.data?.capabilities}
          ownedAsLocal={ownedAsLocal}
        />
      ))}
    </div>
  )
}
