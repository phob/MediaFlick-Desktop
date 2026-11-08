import { CastRow, CrewRow } from "@/components/detail/CastRow"
import { DetailFacts } from "@/components/detail/DetailFacts"
import { DetailHeroLayout } from "@/components/detail/DetailHeroLayout"
import { detailPoster } from "@/components/detail/detail-poster"
import { DetailBackdrop } from "@/components/detail/DetailPrimitives"
import { ItemGrid } from "@/components/ItemGrid"
import type { ReactNode } from "react"
import {
  backdropUrl,
  logoUrl,
  type BookItemDetail,
  type GenericItemDetail,
  type ItemAbout,
} from "@/lib/api"
import { formatRuntime, formatYearOnly } from "@/lib/format"
import type { DetailNavigationState } from "@/lib/navigation"
import { useRouteScroller } from "@/lib/route-scroll"


/** A detail page without a title's playback: folders, music, books. `facts` and `children` add to the hero. */
export function GenericItemPage({
  item,
  about,
  navigationState,
  facts: extraFacts = [],
  children,
}: {
  item: GenericItemDetail | BookItemDetail
  about: ItemAbout | undefined
  navigationState: DetailNavigationState
  facts?: string[]
  children?: ReactNode
}) {
  const scroller = useRouteScroller()
  const backdrop = backdropUrl(item)
  const facts = [
    item.year != null ? String(item.year) : formatYearOnly(item.premiereDate),
    formatRuntime(item.runtimeTicks),
    item.isFolder && item.childCount ? `${item.childCount} ${item.childCount === 1 ? "item" : "items"}` : null,
    ...extraFacts,
  ].filter((fact): fact is string => Boolean(fact))

  return (
    <div className="detail-page relative isolate flex min-h-full min-w-0 flex-col gap-12 pb-16">
      {backdrop && <DetailBackdrop src={backdrop} />}
      <DetailHeroLayout
        compact
        back={{ to: navigationState.from, label: navigationState.label }}
        poster={detailPoster(item)}
        logo={logoUrl(item)}
        title={item.name}
        facts={facts}
        genres={item.genres.map((genre) => ({ label: genre }))}
        overview={item.overview ? (
          <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">{item.overview}</p>
        ) : undefined}
      >
        {children}
      </DetailHeroLayout>

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
