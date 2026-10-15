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
  type ItemAbout,
} from "@/lib/api"
import { artworkShape, type ArtworkShape } from "@/lib/artwork"
import { formatRuntime, formatYearOnly } from "@/lib/format"
import type { DetailNavigationState } from "@/lib/navigation"
import { useRouteScroller } from "@/lib/route-scroll"

const HERO_FRAME: Record<ArtworkShape, HeroArtworkAspect> = {
  portrait: "poster",
  square: "square",
  landscape: "still",
}

export function GenericItemPage({
  item,
  about,
  navigationState,
}: {
  item: GenericItemDetail
  about: ItemAbout | undefined
  navigationState: DetailNavigationState
}) {
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
        compact
        back={{ to: navigationState.from, label: navigationState.label }}
        poster={item.primaryImageTag
          ? {
              src: imageUrl(item, "Primary", DETAIL_POSTER_WIDTH),
              aspect: HERO_FRAME[artworkShape(item.primaryAspect)],
              fit: "contain",
            }
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
            empty="There is nothing in here."
          />
        </section>
      )}

      <CastRow people={about?.people ?? []} />
      <CrewRow people={about?.people ?? []} />

      <div className="max-w-7xl px-6 sm:px-10 lg:px-14">
        <DetailFacts item={item} about={about} />
      </div>
    </div>
  )
}
