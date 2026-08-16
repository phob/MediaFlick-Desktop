import {
  CalendarDays,
  ExternalLink,
  Library,
  Play,
  Plus,
  Star,
} from "lucide-react"
import { useState, type ReactNode } from "react"
import { Link, useLocation, useParams } from "react-router-dom"
import { RequestDialog } from "@/components/seerr/RequestDialog"
import { SeerrStatusBadge } from "@/components/seerr/SeerrStatusBadge"
import { DetailHeroLayout } from "@/components/detail/DetailHeroLayout"
import { DiscoverLetterboxdReviews } from "@/components/detail/LetterboxdReviews"
import {
  DetailCastRail,
  DetailFact,
  DetailFactPanel,
  DetailPageSkeleton,
} from "@/components/detail/DetailPrimitives"
import { PageErrorState } from "@/components/PageHeader"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  seerrImageUrl,
  type SeerrCapabilities,
  type SeerrMediaDetail,
  type SeerrReleaseDate,
} from "@/lib/api"
import { castSearchPath } from "@/lib/cast-search"
import { formatDate, formatLanguage } from "@/lib/format"
import { detailNavigationState } from "@/lib/navigation"
import { useSeerrMedia, useSeerrStatus } from "@/lib/queries"

function runtimeLabel(minutes: number | null) {
  if (!minutes || minutes <= 0) return null
  const hours = Math.floor(minutes / 60)
  return hours ? `${hours}h ${minutes % 60}m` : `${minutes}m`
}

function moneyLabel(value: number | null) {
  if (!value || value <= 0) return null
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 0,
    notation: value >= 1_000_000_000 ? "compact" : "standard",
  }).format(value)
}

function safeHttpUrl(value: string | null) {
  if (!value) return null
  try {
    const url = new URL(value)
    return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : null
  } catch {
    return null
  }
}

function preferredRegion() {
  try {
    return new Intl.Locale(navigator.language).region ?? "US"
  } catch {
    return "US"
  }
}

function regionalReleaseDates(detail: SeerrMediaDetail) {
  const preferred = preferredRegion()
  const local = detail.releaseDates.filter((release) => release.region === preferred)
  const us = detail.releaseDates.filter((release) => release.region === "US")
  const releases = local.length ? local : us.length ? us : detail.releaseDates
  const unique = new Map<string, SeerrReleaseDate>()
  for (const release of releases) {
    unique.set(`${release.region}:${release.type}:${release.date}`, release)
  }
  return [...unique.values()].sort((left, right) => left.date.localeCompare(right.date))
}

const RELEASE_LABELS: Record<SeerrReleaseDate["type"], string> = {
  premiere: "Premiere",
  "limited-cinema": "Limited cinema",
  cinema: "In cinemas",
  digital: "Digital",
  physical: "Physical",
  tv: "Television",
}

function contentRating(detail: SeerrMediaDetail, releases: SeerrReleaseDate[]) {
  const preferred = preferredRegion()
  return (
    detail.contentRatings.find((rating) => rating.region === preferred)?.rating ??
    detail.contentRatings.find((rating) => rating.region === "US")?.rating ??
    releases.find((release) => release.certification)?.certification ??
    null
  )
}

export function Cast({ detail }: { detail: SeerrMediaDetail }) {
  return <DetailCastRail entries={detail.cast.map((person) => ({
    key: `${person.id}-${person.name}`,
    name: person.name,
    role: person.character,
    imageUrl: seerrImageUrl(person.profilePath, "w185"),
    to: castSearchPath({ tmdbId: person.id, name: person.name }),
  }))} />
}

function Seasons({ detail }: { detail: SeerrMediaDetail }) {
  if (!detail.seasons.length) return null
  return (
    <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14">
      <h2 className="section-title">Seasons</h2>
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        {detail.seasons.map((season) => (
          <div
            key={season.seasonNumber}
            className="flex min-w-0 items-center justify-between gap-4 rounded-xl border border-white/5 bg-card/55 px-4 py-3 shadow-lg shadow-black/10"
          >
            <div className="min-w-0">
              <div className="truncate text-sm font-medium">
                {season.name || `Season ${season.seasonNumber}`}
              </div>
              <div className="data-value mt-1 text-muted-foreground">
                {season.episodeCount
                  ? `${season.episodeCount} ${season.episodeCount === 1 ? "episode" : "episodes"}`
                  : "Episode count unknown"}
                {formatDate(season.airDate) ? ` / ${formatDate(season.airDate)}` : ""}
              </div>
            </div>
            <div className="flex shrink-0 flex-col items-end gap-1">
              <SeerrStatusBadge status={season.status} />
              {season.status4k !== "unknown" && (
                <span className="flex items-center gap-1">
                  <span className="data-label text-muted-foreground">4K</span>
                  <SeerrStatusBadge status={season.status4k} />
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}

function requestable(detail: SeerrMediaDetail, capabilities: SeerrCapabilities | null | undefined) {
  const regularAvailable =
    detail.mediaType === "tv" && detail.seasons.length
      ? detail.seasons.some((season) => season.status === "unknown")
      : detail.status === "unknown"
  const fourKAvailable =
    detail.mediaType === "tv" && detail.seasons.length
      ? detail.seasons.some((season) => season.status4k === "unknown")
      : detail.status4k === "unknown"
  return Boolean(
    (capabilities?.[detail.mediaType].request && regularAvailable) ||
      (capabilities?.[detail.mediaType === "movie" ? "movie4k" : "tv4k"].request &&
        fourKAvailable),
  )
}

export default function DiscoverDetail() {
  const location = useLocation()
  const { mediaType, tmdbId } = useParams<{ mediaType: string; tmdbId: string }>()
  const validMediaType = mediaType === "movie" || mediaType === "tv" ? mediaType : undefined
  const parsedId = Number(tmdbId)
  const validId = Number.isSafeInteger(parsedId) && parsedId > 0 ? parsedId : null
  const detail = useSeerrMedia(validMediaType, validId)
  const seerrStatus = useSeerrStatus()
  const [requesting, setRequesting] = useState(false)

  if (!validMediaType || !validId) {
    return <div className="p-6 sm:p-10 lg:p-14"><PageErrorState title="Invalid discovery title" description="That address does not identify a valid Seerr title." /></div>
  }
  if (detail.isPending) return <DetailPageSkeleton />
  if (detail.error && !detail.data) return (
    <div className="p-6 sm:p-10 lg:p-14">
      <PageErrorState
        title="Could not load title details"
        description={detail.error.message}
        action={<Button variant="outline" onClick={() => void detail.refetch()}>Try again</Button>}
      />
    </div>
  )
  if (!detail.data) return <div className="p-6 sm:p-10 lg:p-14"><PageErrorState title="Title unavailable" description="Seerr did not return details for this title." /></div>

  const item = detail.data
  const poster = seerrImageUrl(item.posterPath, "w500")
  const backdrop = seerrImageUrl(item.backdropPath, "w1280")
  const releases = regionalReleaseDates(item)
  const rating = contentRating(item, releases)
  const facts = [
    item.mediaType === "movie" ? "Movie" : item.seriesType || "Series",
    item.year ? String(item.year) : null,
    runtimeLabel(item.runtimeMinutes),
    item.numberOfSeasons
      ? `${item.numberOfSeasons} ${item.numberOfSeasons === 1 ? "season" : "seasons"}`
      : null,
    item.numberOfEpisodes ? `${item.numberOfEpisodes} episodes` : null,
  ].filter(Boolean)
  const nextEpisode = item.nextEpisode
  const nextEpisodeCode =
    nextEpisode?.seasonNumber != null && nextEpisode.episodeNumber != null
      ? `S${nextEpisode.seasonNumber}E${nextEpisode.episodeNumber}`
      : null
  const releaseFallback = releases.length ? null : formatDate(item.releaseDate)
  const language = formatLanguage(item.originalLanguage)
  const homepage = safeHttpUrl(item.homepage)
  const details: { label: string; value: ReactNode }[] = [
    item.originalTitle && item.originalTitle !== item.title
      ? { label: "Original title", value: item.originalTitle }
      : null,
    item.productionStatus ? { label: "Status", value: item.productionStatus } : null,
    language ? { label: "Original language", value: language } : null,
    item.productionCountries.length
      ? {
          label: "Produced in",
          value: item.productionCountries.map((country) => country.name || country.code).join(", "),
        }
      : null,
    item.spokenLanguages.length
      ? {
          label: "Languages",
          value: item.spokenLanguages.map((spoken) => spoken.name || spoken.code).join(", "),
        }
      : null,
    item.studios.length ? { label: "Studios", value: item.studios.join(", ") } : null,
    item.networks.length ? { label: "Networks", value: item.networks.join(", ") } : null,
    item.creators.length ? { label: "Created by", value: item.creators.join(", ") } : null,
    item.directors.length ? { label: "Directed by", value: item.directors.join(", ") } : null,
    item.writers.length ? { label: "Written by", value: item.writers.join(", ") } : null,
    moneyLabel(item.budget) ? { label: "Budget", value: moneyLabel(item.budget) } : null,
    moneyLabel(item.revenue) ? { label: "Revenue", value: moneyLabel(item.revenue) } : null,
    homepage
      ? {
          label: "Official site",
          value: (
            <a
              href={homepage}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-primary hover:underline"
            >
              Open website
              <ExternalLink className="size-3.5" />
            </a>
          ),
        }
      : null,
  ].filter((fact) => fact !== null)

  return (
    <div className="detail-page relative isolate flex min-w-0 flex-col gap-12 pb-16">
      <DetailHeroLayout
        back={{
          to: { pathname: "/discover", search: location.search },
          label: "Back to discovery",
        }}
        backdrop={backdrop}
        poster={poster ? { src: poster, aspect: "poster" } : null}
        title={item.title}
        subtitle={
          item.tagline ? (
            <p className="mt-2 max-w-2xl text-base text-foreground/70 italic">
              {item.tagline}
            </p>
          ) : undefined
        }
        facts={facts}
        metadata={
          <>
            {rating && (
              <Badge variant="outline" className="data-label border-muted-foreground/40 px-1.5 py-0.5">
                {rating}
              </Badge>
            )}
            {item.voteAverage != null && item.voteAverage > 0 && (
              <span className="flex items-center gap-1 text-foreground">
                <Star className="size-3.5 fill-current text-amber-400" aria-hidden />
                {item.voteAverage.toFixed(1)}
                {item.voteCount ? (
                  <span className="text-muted-foreground">
                    ({item.voteCount.toLocaleString()})
                  </span>
                ) : null}
              </span>
            )}
          </>
        }
        genres={item.genres.map((genre) => ({ label: genre }))}
        status={
          <>
            {item.libraryItemId ? <Badge>In your library</Badge> : <SeerrStatusBadge status={item.status} />}
            {item.status4k !== "unknown" && (
              <span className="flex items-center gap-1">
                <span className="data-label text-muted-foreground">4K</span>
                <SeerrStatusBadge status={item.status4k} />
              </span>
            )}
          </>
        }
        overview={
          item.overview ? (
            <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">
              {item.overview}
            </p>
          ) : undefined
        }
      >
        <div className="flex flex-wrap gap-2 pt-1">
          {item.trailer && (
            <Button size="lg" variant="secondary" asChild>
              <a
                href={`https://www.youtube.com/watch?v=${item.trailer.key}`}
                target="_blank"
                rel="noreferrer"
              >
                <Play />
                Watch trailer
              </a>
            </Button>
          )}
          {item.libraryItemId && (
            <Button size="lg" asChild>
              <Link
                to={`/item/${encodeURIComponent(item.libraryItemId)}`}
                state={detailNavigationState(location)}
              >
                <Library />
                Open in library
              </Link>
            </Button>
          )}
          {requestable(item, seerrStatus.data?.capabilities) && (
            <Button
              size="lg"
              variant={item.libraryItemId ? "outline" : "default"}
              onClick={() => setRequesting(true)}
            >
              <Plus />
              Request options
            </Button>
          )}
        </div>
      </DetailHeroLayout>

      <DiscoverLetterboxdReviews mediaType={item.mediaType} tmdbId={item.tmdbId} />

      <Seasons detail={item} />
      <Cast detail={item} />

      <div className="grid max-w-7xl gap-8 px-6 sm:px-10 lg:grid-cols-2 lg:px-14">
        <section className="flex flex-col gap-3">
          <h2 className="section-title">Release schedule</h2>
          <DetailFactPanel>
            {releases.map((release) => (
              <DetailFact key={`${release.type}-${release.date}`} label={RELEASE_LABELS[release.type]}>
                <span className="flex flex-wrap items-center gap-2">
                  <CalendarDays className="size-4 text-primary" />
                  <span className="data-label text-muted-foreground">{release.region}</span>
                  {formatDate(release.date)}
                  {release.certification && <Badge variant="outline">{release.certification}</Badge>}
                </span>
              </DetailFact>
            ))}
            {releaseFallback && (
              <DetailFact label={item.mediaType === "movie" ? "Release" : "First aired"}>
                <span className="flex items-center gap-2">
                  <CalendarDays className="size-4 text-primary" />
                  {releaseFallback}
                </span>
              </DetailFact>
            )}
            {formatDate(item.lastAirDate) && (
              <DetailFact label="Last aired">{formatDate(item.lastAirDate)}</DetailFact>
            )}
            {nextEpisode && (
              <DetailFact label="Next episode">
                <span className="flex flex-col">
                  <span>{[nextEpisodeCode, nextEpisode.name].filter(Boolean).join(" · ")}</span>
                  {formatDate(nextEpisode.airDate) && (
                    <span className="data-value text-muted-foreground">
                      {formatDate(nextEpisode.airDate)}
                    </span>
                  )}
                </span>
              </DetailFact>
            )}
            {!releases.length && !releaseFallback && !nextEpisode && (
              <DetailFact label="Release">No release date has been announced.</DetailFact>
            )}
          </DetailFactPanel>
        </section>

        {details.length > 0 && (
          <section className="flex flex-col gap-3">
            <h2 className="section-title">Details</h2>
            <DetailFactPanel>
              {details.map((fact) => (
                <DetailFact key={fact.label} label={fact.label}>
                  {fact.value}
                </DetailFact>
              ))}
            </DetailFactPanel>
          </section>
        )}
      </div>

      {requesting && <RequestDialog result={item} onClose={() => setRequesting(false)} />}
    </div>
  )
}
