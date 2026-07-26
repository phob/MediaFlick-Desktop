import { useSearchParams } from "react-router-dom"
import { MediaCard } from "@/components/MediaCard"
import { Skeleton } from "@/components/ui/skeleton"
import { PAGE_SIZE } from "@/lib/api"
import { useItems } from "@/lib/queries"

export default function Library() {
  const [params] = useSearchParams()
  const search = params.get("search") ?? ""
  // A search spans the whole library; browsing defaults to movies.
  const kind = params.has("kind") ? (params.get("kind") ?? "") : search ? "" : "Movie"

  const query = {
    search,
    kind,
    genre: params.get("genre") ?? "",
    sort: params.get("sort") ?? "name",
    watched: (params.get("watched") ?? "") as "true" | "false" | "",
    limit: PAGE_SIZE,
    offset: 0,
  }
  const { data, isPending, error } = useItems(query)

  if (error) return <p className="p-6 text-sm text-destructive">{error.message}</p>

  return (
    <div className="p-6">
      {/* TODO(port): windowed grid via `useWindowVirtualizer` sized from `total`
          with a sparse page map + skeletons for unloaded slots, replacing this
          single-page fetch. See .planning/research/silo-server-web.md §4. */}
      <div
        className="grid gap-[var(--card-gap)]"
        style={{ gridTemplateColumns: "repeat(auto-fill, minmax(var(--poster-width), 1fr))" }}
      >
        {isPending
          ? Array.from({ length: 18 }, (_, index) => (
              <Skeleton key={index} className="h-poster-h w-poster-w rounded-lg" />
            ))
          : data.items.map((item) => <MediaCard key={item.id} item={item} />)}
      </div>
      {!isPending && !data.items.length && (
        <p className="text-sm text-muted-foreground">No items match that query.</p>
      )}
    </div>
  )
}
