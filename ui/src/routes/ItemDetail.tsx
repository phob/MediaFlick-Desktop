import { useParams } from "react-router-dom"
import { CastRow } from "@/components/detail/CastRow"
import { DetailActions } from "@/components/detail/DetailActions"
import { DetailFacts } from "@/components/detail/DetailFacts"
import { DetailHero } from "@/components/detail/DetailHero"
import { EpisodeList } from "@/components/detail/EpisodeList"
import { MediaInfo } from "@/components/detail/MediaInfo"
import { MediaCard } from "@/components/MediaCard"
import { Skeleton } from "@/components/ui/skeleton"
import type { ItemDetail as Item, ItemSummary } from "@/lib/api"
import { useChildren, useItem, useMediaInfo, useNextUp } from "@/lib/queries"

function DetailSkeleton() {
  return (
    <div className="flex gap-8 p-6">
      <Skeleton className="hidden h-[330px] w-[220px] rounded-xl sm:block" />
      <div className="flex flex-1 flex-col gap-4">
        <Skeleton className="h-9 w-2/3 max-w-lg" />
        <Skeleton className="h-4 w-48" />
        <Skeleton className="h-20 max-w-3xl" />
        <Skeleton className="h-10 w-72" />
      </div>
    </div>
  )
}

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
  const { data: item, isPending, error } = useItem(id)
  const children = useChildren(id)
  const isSeries = item?.kind === "Series"
  const isContainer = isSeries || item?.kind === "Season"
  // Both of these are server round trips, so they only run for the kind that
  // has an answer: containers have no streams, and only a series has a Next Up.
  const media = useMediaInfo(id, Boolean(item) && !isContainer)
  const nextUp = useNextUp(id, isSeries)

  if (error) return <p className="p-6 text-sm text-destructive">{error.message}</p>
  if (isPending) return <DetailSkeleton />

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
      <DetailHero item={item} episodeCount={episodeCount || null}>
        {isSeries && nextUp.data?.item && <NextUpNote episode={nextUp.data.item} />}
        <DetailActions
          item={item}
          playTarget={playTarget}
          playLabel={playLabelFor(item, playTarget ?? null)}
        />
      </DetailHero>

      {episodes.length > 0 && <EpisodeList episodes={episodes} parentId={item.id} />}
      <SeasonGrid seasons={seasons} />

      <CastRow people={item.people} />

      <div className="grid max-w-7xl gap-8 px-6 sm:px-10 lg:grid-cols-2 lg:px-14">
        <DetailFacts item={item} />
        <MediaInfo sources={media.data?.sources} isPending={!isContainer && media.isPending} />
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
