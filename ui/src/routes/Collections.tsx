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

/** Deterministic hue pair from the collection name so each untitled-art
 * collection gets a stable, distinct gradient without any image request. */
function titleCardPalette(name: string) {
  let hash = 0
  for (let index = 0; index < name.length; index += 1) {
    hash = (hash * 31 + name.charCodeAt(index)) >>> 0
  }
  const hue = hash % 360
  return {
    from: `hsl(${hue} 45% 22%)`,
    via: `hsl(${(hue + 40) % 360} 40% 14%)`,
    accent: `hsl(${(hue + 40) % 360} 60% 62%)`,
  }
}

function CollectionTitleArt({ name }: { name: string }) {
  const palette = titleCardPalette(name)
  return (
    <div
      className="relative flex h-full w-full flex-col justify-end p-3"
      style={{
        backgroundImage: `linear-gradient(155deg, ${palette.from}, ${palette.via} 70%)`,
      }}
    >
      {/* Faint oversized glyph as texture */}
      <Layers
        aria-hidden
        className="absolute -right-2 -top-2 size-20 opacity-10"
        style={{ color: palette.accent }}
      />
      {/* Accent rule tying the card to its palette */}
      <div
        aria-hidden
        className="mb-2 h-px w-8 rounded-full"
        style={{ backgroundColor: palette.accent }}
      />
      <div className="line-clamp-4 text-sm font-semibold leading-snug text-white/90">
        {name}
      </div>
    </div>
  )
}

function CollectionCard({ collection }: { collection: CollectionSummary }) {
  const poster = collectionPoster(collection)
  // Title-art cards carry the name on the artwork; don't repeat it below.
  const showCaptionName = Boolean(poster)
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
          <CollectionTitleArt name={collection.name} />
        )}
      </div>
      <div className="min-w-0 pt-2">
        {showCaptionName && (
          <div className="truncate text-sm font-medium">{collection.name}</div>
        )}
        {collection.itemCount != null && collection.itemCount > 0 && (
          <div className="data-value text-muted-foreground">
            {collection.itemCount} {collection.itemCount === 1 ? "item" : "items"}
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
 * way, a card opens the collection with its owned items. When Seerr knows the
 * full definition, the page loads its missing entries separately.
 */
export default function Collections() {
  const { data, isPending, error } = useCollections()

  return (
    <div className="flex min-w-0 flex-col gap-6 pb-16">
      <PageHeader
        eyebrow="Library"
        title="Collections"
        description="Collections your server carries. Missing movies and series can be requested from their collection page."
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
