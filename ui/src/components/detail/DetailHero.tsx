import { Star } from "lucide-react"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"
import { DetailHeroLayout } from "@/components/detail/DetailHeroLayout"
import { DetailRatingReadout } from "@/components/RatingOverlay"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import {
  DETAIL_POSTER_WIDTH,
  backdropUrl,
  imageUrl,
  logoUrl,
  progressFraction,
  type ItemAbout,
  type ItemDetail,
} from "@/lib/api"
import { formatCommunityRating, formatRuntime, formatYearOnly } from "@/lib/format"
import type { DetailNavigationState } from "@/lib/navigation"

function Breadcrumb({ item, navigationState }: { item: ItemDetail; navigationState: DetailNavigationState }) {
  const links: { to: string; label: string }[] = []
  if (item.seriesId && item.seriesName && item.kind !== "Series") {
    links.push({ to: `/item/${encodeURIComponent(item.seriesId)}`, label: item.seriesName })
  }
  if (item.kind === "Episode" && item.seasonId && item.parentIndexNumber != null) {
    links.push({
      to: `/item/${encodeURIComponent(item.seasonId)}`,
      label: `Season ${item.parentIndexNumber}`,
    })
  }
  if (!links.length) return null

  return (
    <nav className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
      {links.map((link, index) => (
        <span key={link.to} className="flex items-center gap-2">
          {index > 0 && <span aria-hidden>›</span>}
          <Link
            to={link.to}
            state={navigationState}
            className="rounded-sm hover:text-foreground hover:underline"
          >
            {link.label}
          </Link>
        </span>
      ))}
    </nav>
  )
}

function genreHref(genre: string, kind: string) {
  const browseKind = kind === "Movie" ? "Movie" : "Series"
  return `/library?kind=${browseKind}&genre=${encodeURIComponent(genre)}`
}

function JellyfinRatings({ item, about }: { item: ItemDetail; about?: ItemAbout }) {
  const communityRating = formatCommunityRating(item.communityRating)
  return (
    <>
      {communityRating && (
        <span
          className="flex items-center gap-1 text-foreground"
          title={`Jellyfin community rating: ${communityRating} out of 10`}
          aria-label={`Jellyfin community rating ${communityRating} out of 10`}
        >
          <Star className="size-3.5 fill-current text-amber-400" aria-hidden />
          {communityRating}
        </span>
      )}
      {about?.criticRating != null && (
        <span title="Critic score">{Math.round(about.criticRating)}% critics</span>
      )}
    </>
  )
}

export function DetailHero({
  item,
  about,
  aboutPending = false,
  aboutFailed = false,
  episodeCount,
  navigationState,
  children,
}: {
  item: ItemDetail
  about?: ItemAbout
  aboutPending?: boolean
  aboutFailed?: boolean
  episodeCount?: number | null
  navigationState: DetailNavigationState
  children?: ReactNode
}) {
  const episodeCode =
    item.kind === "Episode" && item.parentIndexNumber != null && item.indexNumber != null
      ? `S${item.parentIndexNumber}E${item.indexNumber}`
      : null
  const seasons = item.kind === "Series" && item.childCount ? item.childCount : null
  const facts = [
    episodeCode,
    item.year != null ? String(item.year) : formatYearOnly(item.premiereDate),
    formatRuntime(item.runtimeTicks),
    seasons ? `${seasons} ${seasons === 1 ? "season" : "seasons"}` : null,
    episodeCount ? `${episodeCount} ${episodeCount === 1 ? "episode" : "episodes"}` : null,
  ].filter((fact): fact is string => Boolean(fact))
  const poster = item.primaryImageTag
    ? {
        src: imageUrl(item, "Primary", DETAIL_POSTER_WIDTH),
        aspect: item.kind === "Episode" ? ("still" as const) : ("poster" as const),
        progress: progressFraction(item),
      }
    : null

  return (
    <DetailHeroLayout
      back={{ to: navigationState.from, label: navigationState.label }}
      backdrop={backdropUrl(item)}
      poster={poster}
      logo={logoUrl(item)}
      title={item.name}
      subtitle={
        item.originalTitle && item.originalTitle !== item.name ? (
          <p className="text-sm text-muted-foreground">{item.originalTitle}</p>
        ) : undefined
      }
      breadcrumb={<Breadcrumb item={item} navigationState={navigationState} />}
      facts={facts}
      metadata={
        <>
          {item.officialRating && (
            <Badge variant="outline" className="data-label border-muted-foreground/40 px-1.5 py-0.5">
              {item.officialRating}
            </Badge>
          )}
          <JellyfinRatings item={item} about={about} />
          <DetailRatingReadout item={item} />
        </>
      }
      genres={item.genres.map((genre) => ({
        label: genre,
        to: genreHref(genre, item.kind),
      }))}
      overview={
        aboutPending ? (
          <div className="flex max-w-3xl flex-col gap-2" aria-hidden>
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-4/5" />
          </div>
        ) : about?.overview ? (
          <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">
            {about.overview}
          </p>
        ) : aboutFailed ? (
          <p className="max-w-3xl text-sm text-muted-foreground">
            Details are unavailable while the server cannot be reached.
          </p>
        ) : undefined
      }
    >
      {children}
    </DetailHeroLayout>
  )
}
