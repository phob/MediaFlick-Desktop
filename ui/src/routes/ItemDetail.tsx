import { Navigate, useLocation, useParams, useSearchParams } from "react-router-dom"
import { CastRow } from "@/components/detail/CastRow"
import { DetailActions } from "@/components/detail/DetailActions"
import { DetailFacts } from "@/components/detail/DetailFacts"
import { DetailHero } from "@/components/detail/DetailHero"
import { MediaInfo } from "@/components/detail/MediaInfo"
import { LetterboxdReviews } from "@/components/detail/LetterboxdReviews"
import { DetailPageSkeleton } from "@/components/detail/DetailPrimitives"
import { SeasonBrowser } from "@/components/detail/SeasonBrowser"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import type { ItemDetail as Item, ItemSummary } from "@/lib/api"
import {
  defaultDetailNavigationState,
  readDetailNavigationState,
  type DetailNavigationState,
} from "@/lib/navigation"
import { useChildren, useItem, useItemAbout, useMediaInfo, useNextUp } from "@/lib/queries"
import { seasonRailOrder } from "@/lib/seasons"

function episodeCode(episode: ItemSummary) {
  return episode.parentIndexNumber != null && episode.indexNumber != null
    ? `S${episode.parentIndexNumber}E${episode.indexNumber}`
    : null
}

/**
 * Which episode a series' Play button will start, spelled out under the hero.
 * The button itself only has room for the code.
 */
function NextUpNote({ episode }: { episode: ItemSummary }) {
  return (
    <p className="text-sm text-muted-foreground">
      Next up: {[episodeCode(episode), episode.name].filter(Boolean).join(" · ")}
    </p>
  )
}

export default function ItemDetail() {
  const { id } = useParams<{ id: string }>()
  const location = useLocation()
  const [searchParams, setSearchParams] = useSearchParams()
  const itemQuery = useItem(id)
  const { data: item, isPending, error } = itemQuery
  // The cached row answers instantly; the synopsis, cast, tags, studios, and
  // critic score arrive from this live fetch and are never persisted.
  const about = useItemAbout(id)
  const children = useChildren(id)
  const isSeries = item?.kind === "Series"
  const isContainer = isSeries || item?.kind === "Season"
  // Both of these are server round trips, so they only run for the kind that
  // has an answer: containers have no streams, and only a series has a Next Up.
  const media = useMediaInfo(id, Boolean(item) && !isContainer)
  const nextUp = useNextUp(id, isSeries)

  const childItems = children.data?.items ?? []
  const seasons = seasonRailOrder(childItems.filter((child) => child.kind === "Season"))
  const nextUpItem = isSeries ? (nextUp.data?.item ?? null) : null
  // Which season the episode grid shows: the URL wins, then the season Next Up
  // lives in, then the first regular season. While Next Up is still loading
  // there is no honest default, so the grid holds its skeleton instead of
  // painting season one and jumping.
  const selectedSeason = !isSeries
    ? null
    : (seasons.find((season) => season.id === searchParams.get("season")) ??
      (nextUp.isPending
        ? null
        : (seasons.find((season) => season.id === nextUpItem?.seasonId) ?? seasons[0] ?? null)))
  const seasonChildren = useChildren(selectedSeason?.id)
  const seasonEpisodes = (seasonChildren.data?.items ?? []).filter(
    (child) => child.kind === "Episode",
  )

  if (error && !item) return (
    <div className="p-6 sm:p-10 lg:p-14">
      <PageErrorState
        title="Could not load title details"
        description={error.message}
        action={<Button variant="outline" onClick={() => void itemQuery.refetch()}>Try again</Button>}
      />
    </div>
  )
  if (isPending) return <DetailPageSkeleton />
  if (!item) return <div className="p-6 sm:p-10 lg:p-14"><PageErrorState title="Title unavailable" description="That title is no longer available in your library." /></div>

  // Episodes are browsed on the series page; a season is not a page of its
  // own, so anything still linking one lands on its series with it selected.
  if (item.kind === "Season" && item.seriesId) {
    return (
      <Navigate
        to={`/item/${encodeURIComponent(item.seriesId)}?season=${encodeURIComponent(item.id)}`}
        replace
      />
    )
  }

  const navigationState: DetailNavigationState =
    readDetailNavigationState(location.state) ?? defaultDetailNavigationState(item.kind)

  // A series knows how many seasons it has but not how many episodes; each
  // season row carries its own count, so the total is already on hand.
  const episodeCount = isSeries
    ? seasons.reduce((total, season) => total + (season.childCount ?? 0), 0)
    : 0

  // A series cannot be played itself, so its Play button stands in for the
  // Next Up episode; everything else plays itself and needs no target at all.
  const playTarget = isSeries ? nextUpItem : undefined

  return (
    // `isolate` scopes the hero's backdrop, which is painted at a negative
    // z-index: it has to sit behind every section on this page and in front of the
    // app shell's opaque background, and this stacking context is what pins it
    // between the two.
    <div className="detail-page relative isolate flex min-w-0 flex-col gap-12 pb-16">
      <DetailHero
        item={item}
        about={about.data}
        aboutPending={about.isPending}
        aboutFailed={about.isError}
        episodeCount={episodeCount || null}
        navigationState={navigationState}
      >
        {isSeries && nextUp.data?.item && <NextUpNote episode={nextUp.data.item} />}
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
              // Season switches restate the page rather than advancing it, so
              // Back leaves the series instead of unwinding every click.
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

      {/* Cast is live-only; the row appears when the fetch lands and simply
          stays absent when the server cannot be reached. */}
      <CastRow people={about.data?.people ?? []} />

      <div className="grid max-w-7xl gap-8 px-6 sm:px-10 lg:grid-cols-2 lg:px-14">
        <DetailFacts item={item} about={about.data} />
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
