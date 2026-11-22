import { useQuery } from "@tanstack/react-query"
import { MediaCard } from "@/components/MediaCard"
import { MediaRail } from "@/components/MediaRail"
import { api, type ItemDetail } from "@/lib/api"
import { isTitleKind } from "@/lib/kinds"
import { optionalIdQuery, queryKeys } from "@/lib/query-client"

export function SimilarTitles({ item }: { item: ItemDetail }) {
  const { data } = useQuery({
    ...optionalIdQuery(item.id, queryKeys.similar(item.id), (id) => api.similar(id), isTitleKind(item.kind)),
    staleTime: 30 * 60_000,
    retry: false,
  })
  if (!data?.items.length) return null
  const { items } = data
  return (
    <MediaRail title="More like this" itemCount={items.length} resetKey={items[0].id}>
      {items.map((similar) => (
        <MediaCard key={similar.id} item={similar} className="home-media-card shrink-0 snap-start" />
      ))}
    </MediaRail>
  )
}
