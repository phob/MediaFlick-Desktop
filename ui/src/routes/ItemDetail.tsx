import { useLocation, useParams } from "react-router-dom"
import { CastRow } from "@/components/detail/CastRow"
import { DetailActions } from "@/components/detail/DetailActions"
import { DetailFacts } from "@/components/detail/DetailFacts"
import { DetailHero } from "@/components/detail/DetailHero"
import { EpisodeList } from "@/components/detail/EpisodeList"
import { MediaInfo } from "@/components/detail/MediaInfo"
import { LetterboxdReviews } from "@/components/detail/LetterboxdReviews"
import { DetailPageSkeleton } from "@/components/detail/DetailPrimitives"
import { MediaCard } from "@/components/MediaCard"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import type { ItemDetail as Item, ItemSummary } from "@/lib/api"
import {
  defaultDetailNavigationState,
  readDetailNavigationState,
  type DetailNavigationState,
} from "@/lib/navigation"
import { useChildren, useItem, useItemAbout, useMediaInfo, useNextUp } from "@/lib/queries"

/** Seasons of a series still read best as posters — they are covers, not text. */
function SeasonGrid({ seasons }: { seasons: ItemSummary[] }) {
  if (!seasons.length) return null
  return (
    <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
      <h2 className="section-title">Seasons</h2>
      <div className="flex flex-wrap gap-[var(--card-gap)]">
        {seasons.map((season) => (
          <MediaCard key={season.id} item={season} className="catalog-card" />
        ))}
      </div>
    </section>
  )
}

/**
 * What a season's Play button should start: whatever was left half-watched,
 * otherwise the first episode not yet seen, otherwise the season opener. The
 * episodes are already loaded for the list, so this costs nothing.
 */
function seasonPlayTarget(episodes: ItemSummary[]) {
  return (
    episodes.find((episode) => episode.positionTicks > 0) ??
    episodes.find((episode) => !episode.played) ??
    episodes[0] ??
    null
  )
}

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

  const navigationState: DetailNavigationState =
    readDetailNavigationState(location.state) ?? defaultDetailNavigationState(item.kind)

  const childItems = children.data?.items ?? []
  const episodes = childItems.filter((child) => child.kind === "Episode")
  const seasons = childItems.filter((child) => child.kind === "Season")
  // A series knows how many seasons it has but not how many episodes; each
  // season row carries its own count, so the total is already on hand.
  const episodeCount = isSeries
    ? seasons.reduce((total, season) => total + (season.childCount ?? 0), 0)
    : episodes.length

  // Containers cannot be played themselves, so their Play button stands in for
  // one episode; everything else plays itself and needs no target at all.
  const playTarget = isSeries
    ? (nextUp.data?.item ?? null)
    : item.kind === "Season"
      ? seasonPlayTarget(episodes)
      : undefined

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

      {episodes.length > 0 && <EpisodeList episodes={episodes} parentId={item.id} />}
      <SeasonGrid seasons={seasons} />

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
 * A container's Play button starts one particular episode, so it names it
 * rather than leaving the user to guess which one — and whether it will resume.
 */
function playLabelFor(item: Item, target: ItemSummary | null) {
  if ((item.kind !== "Series" && item.kind !== "Season") || !target) return undefined
  const verb = target.positionTicks > 0 ? "Resume" : "Play"
  const code =
    item.kind === "Season" && target.indexNumber != null
      ? `episode ${target.indexNumber}`
      : episodeCode(target)
  return code ? `${verb} ${code}` : verb
}
