import { Layers } from "lucide-react"
import { Link } from "react-router-dom"
import { PageEmptyState, PageErrorState, PageHeader } from "@/components/PageHeader"
import { Skeleton } from "@/components/ui/skeleton"
import { seerrImageUrl, type CollectionSummary } from "@/lib/api"
import { useCollections } from "@/lib/queries"

function CollectionCard({ collection }: { collection: CollectionSummary }) {
  const poster = seerrImageUrl(collection.posterPath, "w342")
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
 * Every TMDB movie collection the library has movies in. The Companion plugin
 * derives the set from the library itself; when it or Seerr is unavailable the
 * category has no answer and says so instead of showing a broken grid.
 */
export default function Collections() {
  const { data, isPending, error } = useCollections()

  return (
    <div className="flex min-w-0 flex-col gap-6 pb-16">
      <PageHeader
        eyebrow="Library"
        title="Collections"
        description="TMDB movie collections your library is part of. Missing entries can be requested from their collection page."
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
          title="No collections yet"
          description="Collections appear here as your movies are matched against TMDB. New movies are matched in the background, so check back after a sync."
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
