import { SeerrResults } from "@/components/seerr/SeerrResults"
import { useSeerrSearch, useSeerrStatus } from "@/lib/queries"

/**
 * The unified-search tail: what the library does not have, under what it does.
 *
 * It is a *separate* query from the local one on purpose — the local FTS answer
 * renders at SQLite speed and must never be held back by a round trip to Seerr,
 * which is itself proxying TMDB. Results the cache already holds are dropped
 * here rather than shown twice: the shell's (kind, TMDB id) join has already
 * said which those are.
 */
export function NotInYourLibrary({ term }: { term: string }) {
  const status = useSeerrStatus()
  const linked = status.data?.linked ?? false
  const search = useSeerrSearch(term, linked)

  if (!linked || !term.trim()) return null

  const results = search.data?.results.filter((result) => !result.libraryItemId)
  // Nothing to add is not worth a heading, and neither is a Seerr that is
  // merely unreachable: the local results above are still a complete answer.
  if (search.error || (search.isSuccess && !results?.length)) return null

  return (
    <section className="flex flex-col gap-3 pt-8">
      <h2 className="section-title">Not in your library</h2>
      <SeerrResults
        results={results}
        isPending={search.isPending}
        placeholders={4}
        empty="Seerr found nothing else."
      />
    </section>
  )
}
