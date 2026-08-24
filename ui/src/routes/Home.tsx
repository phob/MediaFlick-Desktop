import { Film, Tv } from "lucide-react"
import { useMemo, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { Billboard } from "@/components/Billboard"
import { MediaCard } from "@/components/MediaCard"
import { MediaRail } from "@/components/MediaRail"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  imageUrl,
  LANDSCAPE_WIDTH,
  type CalendarEntry,
  type HomeRow,
  type ItemSummary,
} from "@/lib/api"
import { detailNavigationState } from "@/lib/navigation"
import {
  useBillboard,
  useGenres,
  useHome,
  useHomeResume,
  useItem,
  useItems,
  useReleaseCalendar,
} from "@/lib/queries"
import { cn } from "@/lib/utils"

const VIEW_ALL = new Map<HomeRow["id"], string>([
  ["recent", "/library?kind=Movie,Series&sort=added"],
  ["latest-movies", "/library?kind=Movie&sort=year"],
  ["latest-shows", "/library?kind=Series&sort=year"],
  ["favorites", "/library?favorite=true"],
])

/** Films and shows together, the way every shelf below the fold is mixed. */
const BOTH_KINDS = "Movie,Series"

/** How many genre shelves the page ends with. */
const GENRE_ROWS = 6

/** Home looks far enough ahead to cover a short season and dated movie releases. */
const UPCOMING_DAYS = 90
const UPCOMING_LIMIT = 24

function isoDate(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, "0")
  const day = String(date.getDate()).padStart(2, "0")
  return `${year}-${month}-${day}`
}

function upcomingWindow(now = new Date()) {
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + UPCOMING_DAYS)
  return { start: isoDate(now), end: isoDate(end) }
}

function upcomingIdentity(entry: CalendarEntry) {
  if (entry.kind === "movie") return `movie:${entry.tmdbId ?? entry.tvdbId ?? entry.title}`
  return `series:${entry.seriesTmdbId ?? entry.seriesTvdbId ?? entry.seriesLibraryItemId ?? entry.seriesTitle}`
}

function isSeasonPremiere(entry: CalendarEntry) {
  return entry.kind === "episode" && entry.episode === 1 && entry.season != null && entry.season > 0
}

/**
 * Keeps the calendar's chronological order while removing source duplicates.
 * When a service lists episodes one and two on premiere day, the season-start
 * card represents that drop and the second episode does not sit beside it.
 */
function upcomingEntries(entries: CalendarEntry[]) {
  const eligible = entries
    .filter((entry) => entry.kind === "episode" || entry.dateKind !== "air")
    .sort((left, right) => left.date.localeCompare(right.date))

  const seasonPremieres = new Set(
    eligible
      .filter(isSeasonPremiere)
      .map((entry) => `${upcomingIdentity(entry)}:${entry.season}:${entry.date}`),
  )
  const seen = new Set<string>()
  const results: CalendarEntry[] = []
  for (const entry of eligible) {
    if (
      entry.kind === "episode" &&
      entry.episode !== 1 &&
      seasonPremieres.has(`${upcomingIdentity(entry)}:${entry.season}:${entry.date}`)
    ) {
      continue
    }
    const key = isSeasonPremiere(entry)
      ? `${upcomingIdentity(entry)}:${entry.season}:premiere`
      : `${entry.kind}:${entry.dateKind}:${entry.date}:${entry.tmdbId ?? entry.tvdbId ?? entry.title}`
    if (seen.has(key)) continue
    seen.add(key)
    results.push(entry)
    if (results.length === UPCOMING_LIMIT) break
  }
  return results
}

function upcomingDate(date: string) {
  return new Date(`${date}T12:00:00`).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  })
}

function episodeCode(entry: CalendarEntry) {
  if (entry.season == null || entry.episode == null) return null
  return `S${String(entry.season).padStart(2, "0")}E${String(entry.episode).padStart(2, "0")}`
}

function movieReleaseLabel(entry: CalendarEntry) {
  switch (entry.dateKind) {
    case "cinema":
      return "Cinema release"
    case "physical":
      return "Physical release"
    default:
      return "Digital release"
  }
}

function UpcomingCard({ entry }: { entry: CalendarEntry }) {
  const location = useLocation()
  const newSeason = isSeasonPremiere(entry)
  const itemId = entry.kind === "episode"
    ? newSeason
      ? entry.seriesLibraryItemId
      : entry.libraryItemId ?? entry.seriesLibraryItemId
    : entry.libraryItemId
  const artworkOwner = entry.kind === "episode" ? entry.seriesLibraryItemId : entry.libraryItemId
  const artwork = artworkOwner
    ? [
        imageUrl({ id: artworkOwner, primaryImageTag: null }, "Backdrop", LANDSCAPE_WIDTH, null),
        imageUrl({ id: artworkOwner, primaryImageTag: null }, "Primary", LANDSCAPE_WIDTH, null),
      ]
    : []
  const [imageIndex, setImageIndex] = useState(0)
  const image = artwork[imageIndex]
  const title = newSeason ? entry.seriesTitle ?? entry.title : entry.title
  const code = episodeCode(entry)
  const subtitle = entry.kind === "episode"
    ? newSeason
      ? code ?? entry.seriesTitle ?? "Episode"
      : [entry.seriesTitle, code].filter(Boolean).join(" · ")
    : movieReleaseLabel(entry)
  const destination = itemId ? `/item/${encodeURIComponent(itemId)}` : "/calendar"

  return (
    <article className="signal-card group flex w-landscape-w shrink-0 snap-start flex-col gap-2">
      <div className="media-frame relative h-landscape-h w-landscape-w overflow-hidden rounded-media bg-card ring-1 ring-white/5">
        <Link
          to={destination}
          state={itemId ? detailNavigationState(location) : undefined}
          aria-label={`Open ${title}`}
          className="absolute inset-0 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        >
          {image ? (
            <img
              src={image}
              alt=""
              decoding="async"
              onError={() => setImageIndex((current) => current + 1)}
              className="media-backdrop-image h-full w-full object-cover"
            />
          ) : (
            <span className="flex h-full w-full items-center justify-center px-4 text-center text-sm text-muted-foreground">
              {title}
            </span>
          )}
          <span className="data-label absolute top-0 right-0 z-[4] bg-primary px-2 py-1 leading-none text-primary-foreground">
            {upcomingDate(entry.date)}
          </span>
          {newSeason && (
            <span className="absolute bottom-0 left-1/2 z-[4] -translate-x-1/2 bg-primary px-4 py-2 text-sm font-semibold tracking-wide whitespace-nowrap text-primary-foreground">
              NEW SEASON
            </span>
          )}
        </Link>
      </div>
      <Link
        to={destination}
        state={itemId ? detailNavigationState(location) : undefined}
        className="min-w-0 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <div className="truncate text-sm font-medium transition-colors group-hover:text-primary">
          {title}
        </div>
        <div className="data-value truncate text-muted-foreground">{subtitle}</div>
      </Link>
    </article>
  )
}

function UpcomingRow({ entries }: { entries: CalendarEntry[] }) {
  if (!entries.length) return null
  const first = entries[0]
  return (
    <MediaRail
      title="Upcoming"
      viewAll="/calendar"
      itemCount={entries.length}
      resetKey={`${first.kind}-${first.date}-${first.tmdbId ?? first.tvdbId ?? first.title}`}
    >
      {entries.map((entry, index) => (
        <UpcomingCard
          key={`${entry.kind}-${entry.dateKind}-${entry.date}-${entry.tmdbId ?? entry.tvdbId ?? entry.title}-${index}`}
          entry={entry}
        />
      ))}
    </MediaRail>
  )
}

/**
 * Genres worth leading with where the library has them, in the order a browsing
 * page reads best. Anything not named here still gets a shelf — this only
 * decides which few make the cut when a library has thirty genres and the page
 * has room for five.
 */
const PREFERRED_GENRES = [
  "Action",
  "Comedy",
  "Drama",
  "Science Fiction",
  "Thriller",
  "Documentary",
  "Animation",
  "Horror",
  "Adventure",
  "Crime",
  "Fantasy",
  "Romance",
]

function genreHref(genre: string) {
  return `/library?kind=${BOTH_KINDS}&genre=${encodeURIComponent(genre)}&sort=rating`
}

/** A shelf of one genre, which removes itself when the library has nothing in it. */
function GenreRow({ genre }: { genre: string }) {
  const items = useItems({ kind: BOTH_KINDS, genre, sort: "rating", limit: 20 })
  const results = items.data?.items ?? []
  if (!results.length) return null

  return (
    <MediaRail title={genre} viewAll={genreHref(genre)} itemCount={results.length} resetKey={results[0]?.id}>
      {results.map((item) => (
        <MediaCard key={item.id} item={item} className="home-media-card shrink-0 snap-start" />
      ))}
    </MediaRail>
  )
}

/**
 * The one shelf on the page that answers to what was actually watched. Its seed
 * is whatever sits at the top of Continue Watching, and it recommends by that
 * title's leading genre — which is a genre shelf with an honest heading, not a
 * recommendation engine, and the heading says exactly that much.
 *
 * The genre is resolved by the page rather than here, because the shelf of
 * plain genre rows at the bottom has to know which one is already spoken for.
 */
function BecauseYouWatched({ seed, genre }: { seed: ItemSummary; genre: string }) {
  const items = useItems({ kind: BOTH_KINDS, genre, sort: "rating", limit: 20 })

  // The seed itself, and the show it belongs to, are the two things the user
  // demonstrably does not need recommending.
  const results = (items.data?.items ?? []).filter(
    (item) => item.id !== seed.id && item.id !== seed.seriesId,
  )
  if (results.length < 2) return null

  return (
    <MediaRail
      title={`Because you watched ${seed.seriesName ?? seed.name}`}
      viewAll={genreHref(genre)}
      itemCount={results.length}
      resetKey={results[0]?.id}
    >
      {results.map((item) => (
        <MediaCard key={item.id} item={item} className="home-media-card shrink-0 snap-start" />
      ))}
    </MediaRail>
  )
}

function Row({ row }: { row: HomeRow }) {
  if (!row.items.length) return null
  return (
    <MediaRail
      title={row.title}
      viewAll={VIEW_ALL.get(row.id)}
      itemCount={row.items.length}
      resetKey={row.items[0]?.id}
    >
      {row.items.map((item) => (
        <MediaCard
          key={item.id}
          item={item}
          landscape={row.id === "resume"}
          className="home-media-card shrink-0 snap-start"
        />
      ))}
    </MediaRail>
  )
}

function RowSkeleton({ landscape = false }: { landscape?: boolean }) {
  return (
    <section className="flex flex-col gap-3">
      <Skeleton className="mx-6 h-6 w-44 sm:mx-10 lg:mx-14" />
      <div className="flex gap-[var(--card-gap)] overflow-hidden px-6 sm:px-10 lg:px-14">
        {Array.from({ length: 7 }, (_, index) => (
          <Skeleton
            key={index}
            className={
              landscape
                ? "h-landscape-h w-landscape-w shrink-0 rounded-lg"
                : "h-poster-h w-poster-w shrink-0 rounded-lg"
            }
          />
        ))}
      </div>
    </section>
  )
}

function HomeSkeleton() {
  return (
    <div className="flex h-full flex-col gap-9">
      <div className="relative flex h-1/2 min-h-[30rem] shrink-0 items-end px-10 pb-28">
        <Skeleton className="absolute inset-0 rounded-none" />
        <div className="relative z-10 flex w-full max-w-xl flex-col gap-4">
          <Skeleton className="h-3 w-32" />
          <Skeleton className="h-14 w-3/4" />
          <Skeleton className="h-4 w-52" />
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-10 w-64" />
        </div>
      </div>
      <div className="-mt-20 flex flex-col gap-9 pb-12">
        <RowSkeleton landscape />
        <RowSkeleton />
      </div>
    </div>
  )
}

function EmptyHome() {
  return (
    <div className="flex min-h-full items-center justify-center px-6 py-16">
      <div className="flex max-w-lg flex-col items-center gap-5 text-center">
        <div className="flex size-16 items-center justify-center rounded-full bg-secondary">
          <Film className="size-7 text-muted-foreground" />
        </div>
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold tracking-tight">Your next watch will show up here</h1>
          <p className="text-sm leading-relaxed text-muted-foreground">
            The library may still be syncing. You can start browsing now and this page will fill in
            as your titles arrive.
          </p>
        </div>
        <div className="flex flex-wrap justify-center gap-2">
          <Button asChild>
            <Link to="/library?kind=Movie">
              <Film />
              Browse movies
            </Link>
          </Button>
          <Button variant="secondary" asChild>
            <Link to="/library?kind=Series">
              <Tv />
              Browse series
            </Link>
          </Button>
        </div>
      </div>
    </div>
  )
}

/** The genre shelves to end on, minus whichever one is already on the page. */
function genreRows(available: string[] | undefined, exclude: string | null) {
  if (!available?.length) return []
  const inLibrary = new Set(available)
  const preferred = PREFERRED_GENRES.filter((genre) => inLibrary.has(genre))
  const rest = available.filter((genre) => !PREFERRED_GENRES.includes(genre))
  return [...preferred, ...rest].filter((genre) => genre !== exclude).slice(0, GENRE_ROWS)
}

export default function Home() {
  const home = useHome()
  const enrichedResume = useHomeResume()
  const billboard = useBillboard()
  const favorites = useItems({ favorite: true, sort: "added", limit: 24 })
  const genres = useGenres()
  const [releaseWindow] = useState(upcomingWindow)
  const calendar = useReleaseCalendar(releaseWindow.start, releaseWindow.end)
  const upcoming = useMemo(
    () => upcomingEntries(calendar.data?.entries ?? []),
    [calendar.data?.entries],
  )

  const rows = home.data?.rows.filter((row) => row.items.length > 0) ?? []
  const cachedResume = home.data?.rows.find((row) => row.id === "resume")
  const resumeItems = enrichedResume.data?.items ?? cachedResume?.items ?? []
  // A user can have no half-watched titles but still have server-side Next Up
  // episodes. Keep the empty cached row available as that enrichment's shell.
  const resume = cachedResume && resumeItems.length > 0
    ? { ...cachedResume, items: resumeItems }
    : undefined
  const recent = rows.find((row) => row.id === "recent")
  const latestMovies = rows.find((row) => row.id === "latest-movies")
  const latestShows = rows.find((row) => row.id === "latest-shows")

  // The seed for Because You Watched. Its genres are only on the detail
  // record, which is one local SQLite read — never a server request.
  const seed = resume?.items[0] ?? null
  const seedDetail = useItem(seed?.id)
  const seedGenre = seedDetail.data?.genres?.[0] ?? null

  if (home.error && !home.data) {
    return (
      <div className="p-6 sm:p-10 lg:p-14">
        <PageErrorState
          title="Could not load your home page"
          description={home.error.message}
          action={<Button variant="outline" onClick={() => void home.refetch()}>Try again</Button>}
        />
      </div>
    )
  }

  // The billboard and live Next Up shelf are progressive decoration over the
  // SQLite snapshot. Neither may replace already-available cached cards with a
  // full-page skeleton during startup, navigation, or a background refetch.
  if (home.isPending) return <HomeSkeleton />

  const favoriteRow: HomeRow | null = favorites.data?.items.length
    ? { id: "favorites", title: "My List", items: favorites.data.items }
    : null

  const featured = billboard.data?.items ?? []

  if (!featured.length && !rows.length && !favoriteRow && !upcoming.length) return <EmptyHome />

  return (
    <div className="home-page flex h-full flex-col">
      {featured.length > 0 && <Billboard items={featured} />}

      <div
        className={cn(
          "relative z-10 flex flex-col gap-9 pb-12",
          featured.length > 0 && "-mt-20",
        )}
      >
        {resume && <Row row={resume} />}

        {seed && seedGenre && <BecauseYouWatched seed={seed} genre={seedGenre} />}

        {recent && <Row row={recent} />}
        <UpcomingRow entries={upcoming} />
        {latestMovies && <Row row={latestMovies} />}
        {latestShows && <Row row={latestShows} />}
        {favoriteRow && <Row row={favoriteRow} />}

        {genreRows(genres.data?.genres, seedGenre).map((genre) => (
          <GenreRow key={genre} genre={genre} />
        ))}
      </div>
    </div>
  )
}
