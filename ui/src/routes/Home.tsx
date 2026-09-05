import { Film, Settings2, Tv } from "lucide-react"
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
  type HomeElement,
  type HomeRow,
  type ItemSummary,
} from "@/lib/api"
import { detailNavigationState } from "@/lib/navigation"
import { useBillboard, useHome, useHomeResume, useReleaseCalendar } from "@/lib/queries"
import { cn } from "@/lib/utils"

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
    ) continue
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
    case "cinema": return "Cinema release"
    case "physical": return "Physical release"
    default: return "Digital release"
  }
}

function UpcomingCard({ entry }: { entry: CalendarEntry }) {
  const location = useLocation()
  const newSeason = isSeasonPremiere(entry)
  const itemId = entry.kind === "episode"
    ? newSeason ? entry.seriesLibraryItemId : entry.libraryItemId ?? entry.seriesLibraryItemId
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
    ? newSeason ? code ?? entry.seriesTitle ?? "Episode" : [entry.seriesTitle, code].filter(Boolean).join(" · ")
    : movieReleaseLabel(entry)
  const destination = itemId ? `/item/${encodeURIComponent(itemId)}` : "/calendar"

  return (
    <article className="signal-card group flex w-landscape-w shrink-0 snap-start flex-col gap-2">
      <div className="media-frame relative h-landscape-h w-landscape-w overflow-hidden rounded-media bg-card ring-1 ring-white/5">
        <Link to={destination} state={itemId ? detailNavigationState(location) : undefined} aria-label={`Open ${title}`} className="absolute inset-0 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset">
          {image ? <img src={image} alt="" decoding="async" onError={() => setImageIndex((current) => current + 1)} className="media-backdrop-image h-full w-full object-cover" /> : <span className="flex h-full w-full items-center justify-center px-4 text-center text-sm text-muted-foreground">{title}</span>}
          <span className="data-label absolute top-0 right-0 z-[4] bg-primary px-2 py-1 leading-none text-primary-foreground">{upcomingDate(entry.date)}</span>
          {newSeason && <span className="absolute bottom-0 left-1/2 z-[4] -translate-x-1/2 bg-primary px-4 py-2 text-sm font-semibold tracking-wide whitespace-nowrap text-primary-foreground">NEW SEASON</span>}
        </Link>
      </div>
      <Link to={destination} state={itemId ? detailNavigationState(location) : undefined} className="min-w-0 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <div className="truncate text-sm font-medium transition-colors group-hover:text-primary">{title}</div>
        <div className="data-value truncate text-muted-foreground">{subtitle}</div>
      </Link>
    </article>
  )
}

function UpcomingRow({ entries }: { entries: CalendarEntry[] }) {
  if (!entries.length) return null
  return (
    <MediaRail title="Upcoming" viewAll="/calendar" itemCount={entries.length} resetKey={`${entries[0].kind}-${entries[0].date}`}>
      {entries.map((entry, index) => <UpcomingCard key={`${entry.kind}-${entry.dateKind}-${entry.date}-${entry.tmdbId ?? entry.tvdbId ?? entry.title}-${index}`} entry={entry} />)}
    </MediaRail>
  )
}

function rowViewAll(row: HomeRow) {
  if (row.kind === "genre") return `/library?kind=Movie,Series&genre=${encodeURIComponent(row.id)}&sort=rating`
  if (row.kind === "collection") return `/collections/mine/${encodeURIComponent(row.id)}`
  switch (row.id) {
    case "recentlyAdded": return "/library?kind=Movie,Series&sort=added"
    case "latestMovies": return "/library?kind=Movie&sort=year"
    case "latestShows": return "/library?kind=Series&sort=year"
    case "myList": return "/library?favorite=true"
    default: return undefined
  }
}

function Row({ row, landscape = false }: { row: HomeRow; landscape?: boolean }) {
  if (!row.items.length) return null
  return (
    <MediaRail title={row.title} viewAll={rowViewAll(row)} itemCount={row.items.length} resetKey={row.items[0]?.id}>
      {row.items.map((item) => <MediaCard key={item.id} item={item} landscape={landscape} className="home-media-card shrink-0 snap-start" />)}
    </MediaRail>
  )
}

function WatchingRows({
  continueWatching,
  nextUp,
  combine,
}: {
  continueWatching: ItemSummary[]
  nextUp: ItemSummary[]
  combine: boolean
}) {
  if (combine) {
    return <Row landscape row={{ kind: "builtIn", id: "watching", title: "Watching", items: [...continueWatching, ...nextUp] }} />
  }
  return (
    <>
      <Row landscape row={{ kind: "builtIn", id: "continueWatching", title: "Continue Watching", items: continueWatching }} />
      <Row landscape row={{ kind: "builtIn", id: "nextUp", title: "Next Up", items: nextUp }} />
    </>
  )
}

function RowSkeleton({ landscape = false }: { landscape?: boolean }) {
  return <section className="flex flex-col gap-3"><Skeleton className="mx-6 h-6 w-44 sm:mx-10 lg:mx-14" /><div className="flex gap-[var(--card-gap)] overflow-hidden px-6 sm:px-10 lg:px-14">{Array.from({ length: 7 }, (_, index) => <Skeleton key={index} className={landscape ? "h-landscape-h w-landscape-w shrink-0 rounded-lg" : "h-poster-h w-poster-w shrink-0 rounded-lg"} />)}</div></section>
}

function HomeSkeleton() {
  return <div className="flex h-full flex-col gap-9"><div className="relative flex h-1/2 min-h-[30rem] shrink-0 items-end px-10 pb-28"><Skeleton className="absolute inset-0 rounded-none" /></div><div className="-mt-20 flex flex-col gap-9 pb-12"><RowSkeleton landscape /><RowSkeleton /></div></div>
}

function EmptyHome() {
  return <div className="flex min-h-full items-center justify-center px-6 py-16"><div className="flex max-w-lg flex-col items-center gap-5 text-center"><div className="flex size-16 items-center justify-center rounded-full bg-secondary"><Film className="size-7 text-muted-foreground" /></div><div className="space-y-2"><h1 className="text-2xl font-semibold tracking-tight">No titles available</h1><p className="text-sm leading-relaxed text-muted-foreground">The library may still be syncing. You can start browsing now and this page will fill in as your titles arrive.</p></div><div className="flex flex-wrap justify-center gap-2"><Button asChild><Link to="/library?kind=Movie"><Film />Browse movies</Link></Button><Button variant="secondary" asChild><Link to="/library?kind=Series"><Tv />Browse series</Link></Button></div></div></div>
}

function DisabledHome() {
  return <div className="flex min-h-full items-center justify-center px-6 py-16"><div className="flex max-w-lg flex-col items-center gap-5 text-center"><Settings2 className="size-10 text-muted-foreground" /><div className="space-y-2"><h1 className="text-2xl font-semibold tracking-tight">No shelves enabled</h1><p className="text-sm text-muted-foreground">Choose the shelves you want to see in Home settings.</p></div><Button asChild><Link to="/settings/home">Configure Home</Link></Button></div></div>
}

function matches(row: HomeRow, element: HomeElement) {
  return row.kind === element.kind && row.id === element.id
}

export default function Home() {
  const home = useHome()
  const configuration = home.data?.configuration
  const watching = configuration?.elements.find((element) => element.kind === "builtIn" && element.id === "watching")
  const upcomingElement = configuration?.elements.find((element) => element.kind === "builtIn" && element.id === "upcoming")
  const watchingEnabled = Boolean(watching?.enabled && watching.available)
  const nextUpEnabled = Boolean(watchingEnabled && configuration?.watching.nextUp)
  const resume = useHomeResume(nextUpEnabled)
  const billboardEnabled = Boolean(configuration?.billboard)
  const billboard = useBillboard(billboardEnabled)
  const [releaseWindow] = useState(upcomingWindow)
  const calendar = useReleaseCalendar(releaseWindow.start, releaseWindow.end, Boolean(upcomingElement?.enabled))
  const upcoming = useMemo(() => upcomingEntries(calendar.data?.entries ?? []), [calendar.data?.entries])

  if (home.error && !home.data) return <div className="p-6 sm:p-10 lg:p-14"><PageErrorState title="Could not load your home page" description={home.error.message} action={<Button variant="outline" onClick={() => void home.refetch()}>Try again</Button>} /></div>
  if (home.isPending || !configuration) return <HomeSkeleton />

  const configured = configuration.billboard || configuration.elements.some((element) => element.available && element.enabled)
  if (!configured) return <DisabledHome />

  const continueWatching = configuration.watching.continueWatching
    ? resume.data?.continueWatching ?? home.data.continueWatching
    : []
  const nextUp = configuration.watching.nextUp ? resume.data?.nextUp ?? [] : []
  const featured = billboardEnabled ? billboard.data?.items ?? [] : []
  const hasRows = home.data.rows.length > 0 || continueWatching.length > 0 || nextUp.length > 0 || upcoming.length > 0
  if (!featured.length && !hasRows) return <EmptyHome />

  return <div className="home-page flex h-full flex-col">{featured.length > 0 && <Billboard items={featured} />}<div className={cn("relative z-10 flex flex-col gap-9 pb-12", featured.length > 0 && "-mt-20")}>
    {configuration.elements.filter((element) => element.available && element.enabled).map((element) => {
      if (element.kind === "builtIn" && element.id === "watching") return <WatchingRows key="watching" continueWatching={continueWatching} nextUp={nextUp} combine={configuration.watching.combine} />
      if (element.kind === "builtIn" && element.id === "upcoming") return <UpcomingRow key="upcoming" entries={upcoming} />
      const row = home.data.rows.find((candidate) => matches(candidate, element))
      return row ? <Row key={`${element.kind}:${element.id}`} row={row} /> : null
    })}
  </div></div>
}
