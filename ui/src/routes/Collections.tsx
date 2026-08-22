import { Layers } from "lucide-react"
import { Link } from "react-router-dom"
import { PageEmptyState, PageErrorState, PageHeader } from "@/components/PageHeader"
import { Skeleton } from "@/components/ui/skeleton"
import {
  imageUrl,
  seerrImageUrl,
  type CollectionSummary,
} from "@/lib/api"
import { useCollections } from "@/lib/queries"

function collectionPoster(summary: CollectionSummary) {
  // Native BoxSets carry an image tag and draw through the local image proxy;
  // derived TMDB summaries keep the Seerr poster path.
  if (summary.primaryImageTag) {
    return imageUrl(
      { id: String(summary.id), primaryImageTag: summary.primaryImageTag },
      "Primary",
      342,
    )
  }
  return seerrImageUrl(summary.posterPath, "w342")
}

function CollectionCard({ collection }: { collection: CollectionSummary }) {
  const poster = collectionPoster(collection)
  return (
    <Link
      to={`/collections/${collection.id}`}
      className="catalog-card group flex w-poster-w flex-col rounded-lg"
    >
      <div className="relative h-poster-h w-poster-w overflow-hidden rounded-lg bg-card shadow-lg ring-1 ring-white/10 transition group-hover:ring-white/25">
        {poster ? (
          <img
            src={poster}
            alt=""
            loading="lazy"
            decoding="async"
            className="media-artwork-image h-full w-full object-cover transition group-hover:scale-[1.03]"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-muted-foreground">
            <Layers className="size-8" aria-hidden />
          </div>
        )}
      </div>
      <div className="min-w-0 pt-2">
        <div className="truncate text-sm font-medium">{collection.name}</div>
        {collection.movieCount != null && collection.movieCount > 0 && (
          <div className="data-value text-muted-foreground">
            {collection.movieCount} {collection.movieCount === 1 ? "movie" : "movies"}
          </div>
        )}
      </div>
    </Link>
  )
}

/**
 * The library's collections. When the Companion plugin mirrors TMDB
 * collections into Jellyfin's own BoxSets, this page answers from the server
 * directly; otherwise the plugin's derived TMDB summary stands in. Either
 * way, a card opens the collection with its owned parts and — where Seerr
 * knows the full set — its missing entries.
 */
export default function Collections() {
  const { data, isPending, error } = useCollections()

  return (
    <div className="flex min-w-0 flex-col gap-6 pb-16">
      <PageHeader
        eyebrow="Library"
        title="Collections"
        description="Movie collections your server carries. Missing entries can be requested from their collection page."
      />
      {error && !data ? (
        <PageErrorState
          title="Could not load collections"
          description={error.message}
        />
      ) : isPending ? (
        <div className="flex flex-wrap gap-[var(--card-gap)] px-6 sm:px-10 lg:px-14">
          {Array.from({ length: 8 }, (_, index) => (
            <Skeleton key={index} className="h-poster-h w-poster-w shrink-0 rounded-lg" />
          ))}
        </div>
      ) : !data?.collections.length ? (
        <PageEmptyState
          icon={<Layers className="size-6" />}
          title="No collections found"
          description="Collections appear when the Companion plugin mirrors TMDB collections into Jellyfin, or when your server already has BoxSets."
        />
      ) : (
        <div className="flex flex-wrap gap-[var(--card-gap)] px-6 sm:px-10 lg:px-14">
          {data.collections.map((collection) => (
            <CollectionCard key={collection.id} collection={collection} />
          ))}
        </div>
      )}
    </div>
  )
}
