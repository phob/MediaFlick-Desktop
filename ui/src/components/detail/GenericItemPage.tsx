import { CastRow, CrewRow } from "@/components/detail/CastRow"
import { DetailFacts } from "@/components/detail/DetailFacts"
import { DetailHeroLayout, type HeroArtworkAspect } from "@/components/detail/DetailHeroLayout"
import { DetailBackdrop } from "@/components/detail/DetailPrimitives"
import { ItemGrid } from "@/components/ItemGrid"
import {
  backdropUrl,
  DETAIL_POSTER_WIDTH,
  imageUrl,
  logoUrl,
  type GenericItemDetail,
} from "@/lib/api"
import { formatRuntime, formatYearOnly } from "@/lib/format"
import type { DetailNavigationState } from "@/lib/navigation"
import { useItemAbout } from "@/lib/queries"
import { useRouteScroller } from "@/lib/route-scroll"

function artworkAspect(primaryAspect: number | null): HeroArtworkAspect {
  if (primaryAspect == null || primaryAspect < 0.8) return "poster"
  return primaryAspect >= 1.3 ? "still" : "square"
}

/**
 * The details page for every item that is not a movie, show, season or
 * episode: artwork, facts, synopsis, credits and, for a folder, its contents.
 * It deliberately offers no playback, favorites or ratings.
 */
export function GenericItemPage({
  item,
  navigationState,
}: {
  item: GenericItemDetail
  navigationState: DetailNavigationState
}) {
  const about = useItemAbout(item.id)
  const scroller = useRouteScroller()
  const backdrop = backdropUrl(item)
  const facts = [
    item.year != null ? String(item.year) : formatYearOnly(item.premiereDate),
    formatRuntime(item.runtimeTicks),
    item.isFolder && item.childCount ? `${item.childCount} ${item.childCount === 1 ? "item" : "items"}` : null,
  ].filter((fact): fact is string => Boolean(fact))

  return (
    <div className="detail-page relative isolate flex min-h-full min-w-0 flex-col gap-12 pb-16">
      {backdrop && <DetailBackdrop src={backdrop} />}
      <DetailHeroLayout
        back={{ to: navigationState.from, label: navigationState.label }}
        poster={item.primaryImageTag
          ? { src: imageUrl(item, "Primary", DETAIL_POSTER_WIDTH), aspect: artworkAspect(item.primaryAspect) }
          : null}
        logo={logoUrl(item)}
        title={item.name}
        facts={facts}
        genres={item.genres.map((genre) => ({ label: genre }))}
        overview={item.overview ? (
          <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">{item.overview}</p>
        ) : undefined}
      />

      {item.isFolder && (
        <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
          <h2 className="section-title">Contents</h2>
          <ItemGrid
            source={{ scope: "children", itemId: item.id }}
            scroller={scroller}
            empty="This folder is empty."
          />
        </section>
      )}

      <CastRow people={about.data?.people ?? []} />
      <CrewRow people={about.data?.people ?? []} />

      <div className="max-w-7xl px-6 sm:px-10 lg:px-14">
        <DetailFacts item={item} about={about.data} />
      </div>
    </div>
  )
}
