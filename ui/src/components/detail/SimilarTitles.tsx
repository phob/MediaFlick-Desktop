import { MediaCard } from "@/components/MediaCard"
import { MediaRail } from "@/components/MediaRail"
import type { ItemDetail } from "@/lib/api"
import { isTitleKind } from "@/lib/kinds"
import { useSimilarItems } from "@/lib/queries"

/**
 * Jellyfin's similar titles for a film or series. Optional garnish: while it
 * loads, when it fails and when the server finds nothing, the page simply has
 * no rail.
 */
export function SimilarTitles({ item }: { item: ItemDetail }) {
  const { data } = useSimilarItems(item.id, isTitleKind(item.kind))
  const items = data?.items ?? []
  if (!items.length) return null
  return (
    <MediaRail title="More like this" itemCount={items.length} resetKey={items[0]?.id}>
      {items.map((similar) => (
        <MediaCard key={similar.id} item={similar} className="home-media-card shrink-0 snap-start" />
      ))}
    </MediaRail>
  )
}
