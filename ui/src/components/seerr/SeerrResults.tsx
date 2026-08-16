import { SearchX } from "lucide-react"
import { PageEmptyState, PageErrorState } from "@/components/PageHeader"
import { SeerrCard } from "@/components/seerr/SeerrCard"
import { Skeleton } from "@/components/ui/skeleton"
import type { SeerrResult } from "@/lib/api"
import { discoveryCardKey } from "@/lib/discovery"
import { useSeerrStatus } from "@/lib/queries"

/**
 * A wrapped row of Seerr results. Not virtualized on purpose: a Seerr page is
 * twenty titles, and the poster wall the library grid windows for is thousands.
 */
export function SeerrResults({
  results,
  isPending,
  error,
  empty = "Nothing found.",
  placeholders = 6,
  resultSetKey,
}: {
  results: SeerrResult[] | undefined
  isPending?: boolean
  error?: Error | null
  empty?: string
  placeholders?: number
  resultSetKey?: string
}) {
  const status = useSeerrStatus()

  if (error && !results?.length) return <PageErrorState title="Could not load discovery results" description={error.message} />
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
        <SeerrCard
          key={
            resultSetKey
              ? discoveryCardKey(resultSetKey, result)
              : `${result.mediaType}-${result.tmdbId}`
          }
          result={result}
          capabilities={status.data?.capabilities}
        />
      ))}
    </div>
  )
}
