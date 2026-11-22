import { SeerrResults } from "@/components/seerr/SeerrResults"
import { useSeerrSearch, useSeerrStatus } from "@/lib/seerr-queries"

export function NotInYourLibrary({ term }: { term: string }) {
  const status = useSeerrStatus()
  const linked = status.data?.linked ?? false
  const search = useSeerrSearch(term, linked)

  if (!linked || !term.trim()) return null

  const results = search.data?.results.filter((result) => !result.libraryItemId)
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
