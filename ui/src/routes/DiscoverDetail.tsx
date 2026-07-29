import {
  ArrowLeft,
  CalendarDays,
  ExternalLink,
  Library,
  Play,
  Plus,
  Star,
  UserRound,
} from "lucide-react"
import { useState, type ReactNode } from "react"
import { Link, useParams } from "react-router-dom"
import { RequestDialog } from "@/components/seerr/RequestDialog"
import { SeerrStatusBadge } from "@/components/seerr/SeerrStatusBadge"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  seerrImageUrl,
  type SeerrCapabilities,
  type SeerrMediaDetail,
  type SeerrReleaseDate,
} from "@/lib/api"
import { formatDate, formatLanguage } from "@/lib/format"
import { useSeerrMedia, useSeerrStatus } from "@/lib/queries"

function DetailSkeleton() {
  return (
    <div className="flex min-h-full flex-col">
      <div className="flex min-h-[31rem] gap-8 px-6 pt-16 sm:px-10 lg:px-14">
        <Skeleton className="hidden h-[330px] w-[220px] sm:block" />
        <div className="flex flex-1 flex-col gap-4">
          <Skeleton className="h-4 w-32" />
          <Skeleton className="h-12 w-2/3 max-w-2xl" />
          <Skeleton className="h-4 w-72" />
          <Skeleton className="h-24 max-w-3xl" />
          <Skeleton className="h-11 w-72" />
        </div>
      </div>
    </div>
  )
}

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

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[8.5rem_1fr] gap-4 py-2.5 text-sm">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  )
}

function Cast({ detail }: { detail: SeerrMediaDetail }) {
  if (!detail.cast.length) return null
  return (
    <section className="flex min-w-0 flex-col gap-4">
      <h2 className="section-title px-6 sm:px-10 lg:px-14">Cast</h2>
      <div className="media-strip flex gap-6 overflow-x-auto px-6 pb-3 sm:px-10 lg:px-14">
        {detail.cast.map((person) => {
          const image = seerrImageUrl(person.profilePath, "w185")
          return (
            <figure
              key={`${person.id}-${person.name}`}
              className="flex w-28 shrink-0 flex-col items-center gap-2 text-center"
            >
              {image ? (
                <img
                  src={image}
                  alt=""
                  decoding="async"
                  className="size-24 rounded-full object-cover ring-1 ring-white/10"
                />
              ) : (
                <div className="grid size-24 place-items-center rounded-full bg-card text-muted-foreground ring-1 ring-white/10">
                  <UserRound className="size-8" />
                </div>
              )}
              <figcaption className="flex flex-col gap-0.5">
                <span className="text-xs leading-tight">{person.name}</span>
                {person.character && (
                  <span className="text-xs leading-tight text-muted-foreground">
                    {person.character}
                  </span>
                )}
              </figcaption>
            </figure>
          )
        })}
      </div>
    </section>
  )
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
            className="flex min-w-0 items-center justify-between gap-4 border border-white/5 bg-card/55 px-4 py-3 shadow-lg shadow-black/10"
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
  const { mediaType, tmdbId } = useParams<{ mediaType: string; tmdbId: string }>()
  const validMediaType = mediaType === "movie" || mediaType === "tv" ? mediaType : undefined
  const parsedId = Number(tmdbId)
  const validId = Number.isSafeInteger(parsedId) && parsedId > 0 ? parsedId : null
  const detail = useSeerrMedia(validMediaType, validId)
  const seerrStatus = useSeerrStatus()
  const [posterFailed, setPosterFailed] = useState(false)
  const [backdropFailed, setBackdropFailed] = useState(false)
  const [requesting, setRequesting] = useState(false)

  if (!validMediaType || !validId) {
    return <p className="p-6 text-sm text-destructive">That is not a Seerr title.</p>
  }
  if (detail.isPending) return <DetailSkeleton />
  if (detail.error) return <p className="p-6 text-sm text-destructive">{detail.error.message}</p>
  if (!detail.data) return null

  const item = detail.data
  const poster = posterFailed ? null : seerrImageUrl(item.posterPath, "w500")
  const backdrop = backdropFailed ? null : seerrImageUrl(item.backdropPath, "w1280")
  const releases = regionalReleaseDates(item)
  const rating = contentRating(item, releases)
  const facts = [
    item.mediaType === "movie" ? "Film" : item.seriesType || "Series",
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
      <header className="relative min-h-[31rem] overflow-hidden">
        {backdrop && (
          <>
            <img
              src={backdrop}
              alt=""
              decoding="async"
              onError={() => setBackdropFailed(true)}
              className="absolute inset-0 -z-10 h-full w-full object-cover"
            />
            <div className="absolute inset-0 -z-9 bg-linear-to-r from-background via-background/80 to-background/20" />
            <div className="absolute inset-0 -z-9 bg-linear-to-t from-background via-transparent to-background/30" />
          </>
        )}
        <div className="flex gap-8 px-6 pt-10 pb-14 sm:px-10 lg:px-14">
          {poster && (
            <img
              src={poster}
              alt=""
              decoding="async"
              onError={() => setPosterFailed(true)}
              className="hidden aspect-2/3 w-[220px] shrink-0 self-start object-cover shadow-2xl shadow-black/60 ring-1 ring-white/10 sm:block"
            />
          )}
          <div className="flex min-w-0 max-w-4xl flex-1 flex-col gap-4">
            <Button variant="ghost" size="sm" className="w-fit px-0 text-muted-foreground" asChild>
              <Link to="/discover">
                <ArrowLeft />
                Back to discovery
              </Link>
            </Button>
            <div>
              <h1 className="text-4xl leading-[0.98] font-black tracking-[-0.04em] text-balance drop-shadow-lg sm:text-5xl lg:text-6xl">
                {item.title}
              </h1>
              {item.tagline && (
                <p className="mt-3 max-w-2xl text-base text-foreground/70 italic">
                  {item.tagline}
                </p>
              )}
            </div>

            <div className="data-value flex flex-wrap items-center gap-x-2 gap-y-2 text-muted-foreground">
              {facts.map((fact, index) => (
                <span key={fact} className="flex items-center gap-2">
                  {index > 0 && <span className="text-primary/45">/</span>}
                  {fact}
                </span>
              ))}
              {rating && <Badge variant="outline">{rating}</Badge>}
              {item.voteAverage != null && item.voteAverage > 0 && (
                <span className="flex items-center gap-1 text-foreground">
                  <Star className="size-3.5 fill-current text-amber-400" />
                  {item.voteAverage.toFixed(1)}
                  {item.voteCount ? (
                    <span className="text-muted-foreground">
                      ({item.voteCount.toLocaleString()})
                    </span>
                  ) : null}
                </span>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              {item.genres.map((genre) => (
                <Badge key={genre} variant="secondary">
                  {genre}
                </Badge>
              ))}
              <SeerrStatusBadge status={item.status} />
              {item.status4k !== "unknown" && (
                <span className="flex items-center gap-1">
                  <span className="data-label text-muted-foreground">4K</span>
                  <SeerrStatusBadge status={item.status4k} />
                </span>
              )}
            </div>

            {item.overview && (
              <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">
                {item.overview}
              </p>
            )}

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
                  <Link to={`/item/${encodeURIComponent(item.libraryItemId)}`}>
                    <Library />
                    Open in your library
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
          </div>
        </div>
      </header>

      <div className="grid max-w-7xl gap-8 px-6 sm:px-10 lg:grid-cols-2 lg:px-14">
        <section className="flex flex-col gap-3">
          <h2 className="section-title">Release schedule</h2>
          <div className="divide-y divide-border/60 border border-white/5 bg-card/55 px-4 py-1 shadow-lg shadow-black/10">
            {releases.map((release) => (
              <Fact key={`${release.type}-${release.date}`} label={RELEASE_LABELS[release.type]}>
                <span className="flex flex-wrap items-center gap-2">
                  <CalendarDays className="size-4 text-primary" />
                  <span className="data-label text-muted-foreground">{release.region}</span>
                  {formatDate(release.date)}
                  {release.certification && <Badge variant="outline">{release.certification}</Badge>}
                </span>
              </Fact>
            ))}
            {releaseFallback && (
              <Fact label={item.mediaType === "movie" ? "Release" : "First aired"}>
                <span className="flex items-center gap-2">
                  <CalendarDays className="size-4 text-primary" />
                  {releaseFallback}
                </span>
              </Fact>
            )}
            {formatDate(item.lastAirDate) && (
              <Fact label="Last aired">{formatDate(item.lastAirDate)}</Fact>
            )}
            {nextEpisode && (
              <Fact label="Next episode">
                <span className="flex flex-col">
                  <span>{[nextEpisodeCode, nextEpisode.name].filter(Boolean).join(" · ")}</span>
                  {formatDate(nextEpisode.airDate) && (
                    <span className="data-value text-muted-foreground">
                      {formatDate(nextEpisode.airDate)}
                    </span>
                  )}
                </span>
              </Fact>
            )}
            {!releases.length && !releaseFallback && !nextEpisode && (
              <p className="py-4 text-sm text-muted-foreground">
                No release date has been announced.
              </p>
            )}
          </div>
        </section>

        {details.length > 0 && (
          <section className="flex flex-col gap-3">
            <h2 className="section-title">Details</h2>
            <dl className="divide-y divide-border/60 border border-white/5 bg-card/55 px-4 py-1 shadow-lg shadow-black/10">
              {details.map((fact) => (
                <Fact key={fact.label} label={fact.label}>
                  {fact.value}
                </Fact>
              ))}
            </dl>
          </section>
        )}
      </div>

      <Cast detail={item} />
      <Seasons detail={item} />

      {requesting && <RequestDialog result={item} onClose={() => setRequesting(false)} />}
    </div>
  )
}
