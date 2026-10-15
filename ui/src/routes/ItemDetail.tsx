import { useState } from "react"
import { concealEpisode, useSpoilerProtection } from "@/lib/viewing"
import { Layers } from "lucide-react"
import { Navigate, useLocation, useParams, useSearchParams, Link } from "react-router-dom"
import { CastRow, CrewRow } from "@/components/detail/CastRow"
import { DetailActions } from "@/components/detail/DetailActions"
import { DetailFacts } from "@/components/detail/DetailFacts"
import { DetailHero } from "@/components/detail/DetailHero"
import { MediaInfo } from "@/components/detail/MediaInfo"
import { LetterboxdReviews } from "@/components/detail/LetterboxdReviews"
import { DetailPageSkeleton, DetailBackdrop } from "@/components/detail/DetailPrimitives"
import { GenericItemPage } from "@/components/detail/GenericItemPage"
import { SeasonBrowser } from "@/components/detail/SeasonBrowser"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import { backdropUrl, type ItemAbout, type ItemDetail as Item, type ItemSummary } from "@/lib/api"
import {
  defaultDetailNavigationState,
  readDetailNavigationState,
  type DetailNavigationState,
} from "@/lib/navigation"
import { useChildren, useCollectionSettings, useItem, useItemAbout, useMediaInfo, useMovieCollection, useNextUp } from "@/lib/queries"
import { seasonRailOrder } from "@/lib/seasons"

function episodeCode(episode: ItemSummary) {
  return episode.parentIndexNumber != null && episode.indexNumber != null
    ? `S${episode.parentIndexNumber}E${episode.indexNumber}`
    : null
}

function CollectionChip({ tmdbId }: { tmdbId: number }) {
  const settings = useCollectionSettings()
  const { data } = useMovieCollection(tmdbId, settings.data?.effectiveMode === "mediaFlick")
  const collection = data?.collection
  if (!collection) return null
  return (
    <Link
      to={`/collections/franchises/${collection.id}`}
      className="inline-flex w-fit items-center gap-1.5 rounded-full border border-edge bg-card px-3 py-1 text-xs text-muted-foreground shadow-lg shadow-black/40 transition hover:border-white/25 hover:text-foreground"
    >
      <Layers className="size-3" aria-hidden />
      Part of&nbsp;<span className="font-medium text-foreground">{collection.name}</span>
    </Link>
  )
}

/**
 * Which episode a series' Play button will start, spelled out under the hero.
 * The button itself only has room for the code.
 */
function NextUpNote({ episode }: { episode: ItemSummary }) {
  const hidden = useSpoilerProtection(episode)
  return (
    <p className="text-sm text-muted-foreground">
      Next up: {[episodeCode(episode), hidden ? null : episode.name].filter(Boolean).join(" · ")}
    </p>
  )
}

export default function ItemDetail() {
  const { id } = useParams<{ id: string }>()
  const location = useLocation()
  const itemQuery = useItem(id)
  const { data: item, isPending, error } = itemQuery
  const about = useItemAbout(id)
  const children = useChildren(id, item?.layout === "title")

  if (error && !item) return (
    <div className="p-6 sm:p-10 lg:p-14">
      <PageErrorState
        title="Could not load details"
        description={error.message}
        action={<Button variant="outline" onClick={() => void itemQuery.refetch()}>Try again</Button>}
      />
    </div>
  )
  if (isPending) return <DetailPageSkeleton />
  if (!item) return <div className="p-6 sm:p-10 lg:p-14"><PageErrorState title="Not available" description="This is no longer available on your server." /></div>

  const navigationState: DetailNavigationState =
    readDetailNavigationState(location.state) ?? defaultDetailNavigationState(item)
  switch (item.layout) {
    case "generic":
      return <GenericItemPage key={item.id} item={item} about={about.data} navigationState={navigationState} />
    case "title":
      return (
        <TitleItemPage
          key={item.id}
          item={item}
          about={about.data}
          aboutPending={about.isPending}
          aboutFailed={about.isError}
          childItems={children.data?.items ?? []}
          navigationState={navigationState}
        />
      )
    default:
      return item satisfies never
  }
}

function TitleItemPage({
  item: originalItem,
  about,
  aboutPending,
  aboutFailed,
  childItems,
  navigationState,
}: {
  item: Item
  about: ItemAbout | undefined
  aboutPending: boolean
  aboutFailed: boolean
  childItems: ItemSummary[]
  navigationState: DetailNavigationState
}) {
  const id = originalItem.id
  const [searchParams, setSearchParams] = useSearchParams()
  const [revealed, setRevealed] = useState(false)
  const hidden = useSpoilerProtection(originalItem) && !revealed
  const item = hidden ? concealEpisode(originalItem) : originalItem
  const isSeries = item.kind === "Series"
  const isContainer = isSeries || item.kind === "Season"
  const media = useMediaInfo(id, !isContainer)
  const nextUp = useNextUp(id, isSeries)

  const seasons = seasonRailOrder(childItems.filter((child) => child.kind === "Season"))
  const nextUpItem = isSeries ? (nextUp.data?.item ?? null) : null
  // Which season the episode grid shows: the URL wins, then the season Next Up
  // lives in, then the first regular season. While Next Up is still loading
  // there is no honest default, so the grid holds its skeleton instead of
  // painting season one and jumping. A single season has nowhere to jump to.
  const selectedSeason = !isSeries
    ? null
    : (seasons.find((season) => season.id === searchParams.get("season")) ??
      (nextUp.isPending
        ? (seasons.length === 1 ? seasons[0] : null)
        : (seasons.find((season) => season.id === nextUpItem?.seasonId) ?? seasons[0] ?? null)))
  const seasonChildren = useChildren(selectedSeason?.id)
  const seasonEpisodes = (seasonChildren.data?.items ?? []).filter(
    (child) => child.kind === "Episode",
  )

  if (item.kind === "Season" && item.seriesId) {
    return (
      <Navigate
        to={`/item/${encodeURIComponent(item.seriesId)}?season=${encodeURIComponent(item.id)}`}
        replace
      />
    )
  }

  const episodeCount = isSeries
    ? seasons.reduce((total, season) => total + (season.childCount ?? 0), 0)
    : 0
  const backdrop = backdropUrl(item)

  const playTarget = isSeries ? nextUpItem : undefined

  return (
    // `isolate` scopes the page backdrop, which is painted at a negative
    // z-index: it has to sit behind every section on this page and in front of the
    // app shell's opaque background, and this stacking context is what pins it
    // between the two. `min-h-full` stretches short pages to the viewport so
    // the backdrop reaches the window's bottom edge instead of stopping with
    // the content.
    <div className="detail-page relative isolate flex min-h-full min-w-0 flex-col gap-12 pb-16">
      {!hidden && backdrop && <DetailBackdrop src={backdrop} />}
      {hidden && <Button className="relative z-10" variant="outline" onClick={() => setRevealed(true)}>Reveal episode spoilers</Button>}
      <DetailHero
        item={item}
        about={hidden ? undefined : about}
        aboutPending={aboutPending}
        aboutFailed={aboutFailed}
        episodeCount={episodeCount || null}
        navigationState={navigationState}
      >
        {isSeries && nextUp.data?.item && <NextUpNote episode={nextUp.data.item} />}
        {!isSeries && item.providerIds.tmdb && (
          <CollectionChip tmdbId={Number(item.providerIds.tmdb)} />
        )}
        <DetailActions
          item={item}
          playTarget={playTarget}
          playLabel={playLabelFor(item, playTarget ?? null)}
        />
      </DetailHero>

      <LetterboxdReviews item={item} />

      {isSeries && seasons.length > 0 && (
        <SeasonBrowser
          seasons={seasons}
          selectedSeason={selectedSeason}
          onSelect={(season) =>
            setSearchParams(
              (params) => {
                params.set("season", season.id)
                return params
              },
              { replace: true },
            )
          }
          episodes={seasonEpisodes}
          episodesPending={!selectedSeason || seasonChildren.isPending}
          episodesError={seasonChildren.error}
          onRetry={() => void seasonChildren.refetch()}
          nextUpEpisodeId={nextUpItem?.id ?? null}
        />
      )}

      <CastRow people={hidden ? [] : about?.people ?? []} />
      <CrewRow people={hidden ? [] : about?.people ?? []} />

      <div className="grid max-w-7xl gap-8 px-6 sm:px-10 lg:grid-cols-2 lg:px-14">
        <DetailFacts item={item} about={hidden ? undefined : about} />
        <MediaInfo
          itemId={item.id}
          sources={media.data?.sources}
          preference={media.data?.playbackPreference}
          isPending={!isContainer && media.isPending}
        />
      </div>
    </div>
  )
}

/**
 * A series' Play button starts one particular episode, so it names it rather
 * than leaving the user to guess which one — and whether it will resume.
 */
function playLabelFor(item: Item, target: ItemSummary | null) {
  if (item.kind !== "Series" || !target) return undefined
  const verb = target.positionTicks > 0 ? "Resume" : "Play"
  const code = episodeCode(target)
  return code ? `${verb} ${code}` : verb
}
