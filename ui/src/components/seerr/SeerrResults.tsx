import { SearchX } from "lucide-react"
import { PageEmptyState } from "@/components/PageHeader"
import { SeerrCard } from "@/components/seerr/SeerrCard"
import { Skeleton } from "@/components/ui/skeleton"
import type { SeerrResult } from "@/lib/api"

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
}: {
  results: SeerrResult[] | undefined
  isPending?: boolean
  error?: Error | null
  empty?: string
  placeholders?: number
}) {
  if (error) return <p className="py-4 text-sm text-destructive">{error.message}</p>
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
        <SeerrCard key={`${result.mediaType}-${result.tmdbId}`} result={result} />
      ))}
    </div>
  )
}
