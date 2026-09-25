import { Check, CircleAlert, Film, Settings2, Tv } from "lucide-react"
import { Fragment, useMemo, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { Billboard } from "@/components/Billboard"
import { MediaCard } from "@/components/MediaCard"
import { MediaRail } from "@/components/MediaRail"
import { PageErrorState } from "@/components/PageHeader"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  api,
  imageUrl,
  type CalendarEntry,
  type HomeElement,
  type HomeRow,
  type ItemSummary,
} from "@/lib/api"
import { detailNavigationState } from "@/lib/navigation"
import { useBillboard, useHome, useHomeResume, useReleaseCalendar } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { useViewing } from "@/lib/viewing"

const UPCOMING_DAYS = 90
/** How far back the shelf looks for releases that should have arrived. */
const RECENT_DAYS = 30
const UPCOMING_LIMIT = 24
/** The share of the shelf given to recent releases when both sides can fill it. */
const RECENT_SHARE = 0.3

type ReleaseStatus = "downloaded" | "missing"

interface UpcomingShelfEntry {
  entry: CalendarEntry
  /** Dated before today. */
  past: boolean
  /** Set only for past releases; a future date has nothing to be missing yet. */
  status: ReleaseStatus | null
}

function isoDate(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, "0")
  const day = String(date.getDate()).padStart(2, "0")
  return `${year}-${month}-${day}`
}

function upcomingWindow(now = new Date()) {
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - RECENT_DAYS)
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + UPCOMING_DAYS)
  return { start: isoDate(start), today: isoDate(now), end: isoDate(end) }
}

function upcomingIdentity(entry: CalendarEntry) {
  if (entry.kind === "movie") return `movie:${entry.tmdbId ?? entry.tvdbId ?? entry.title}`
  return `series:${entry.seriesTmdbId ?? entry.seriesTvdbId ?? entry.seriesLibraryItemId ?? entry.seriesTitle}`
}

function isSeasonPremiere(entry: CalendarEntry) {
  return entry.kind === "episode" && entry.episode === 1 && entry.season != null && entry.season > 0
}

function seasonDay(entry: CalendarEntry) {
  return `${upcomingIdentity(entry)}:${entry.season}:${entry.date}`
}

/**
 * The Companion's file flags come from a daily snapshot, so a library match
 * also counts: it shows a download that landed since the last refresh.
 */
function isDownloaded(entry: CalendarEntry) {
  return entry.hasFile || entry.libraryItemId != null
}

function shelfKey(entry: CalendarEntry, past: boolean, premieres: Set<string>) {
  // Same-day later episodes of a season premiere fold into its one card.
  if (entry.kind === "episode" && premieres.has(seasonDay(entry))) return `${upcomingIdentity(entry)}:${entry.season}:premiere`
  // A released movie is one card at its latest release, not one per channel.
  if (entry.kind === "movie" && past) return `released:${upcomingIdentity(entry)}`
  return `${entry.kind}:${entry.dateKind}:${entry.date}:${entry.tmdbId ?? entry.tvdbId ?? entry.title}`
}

function releaseStatus(entries: CalendarEntry[]): ReleaseStatus | null {
  if (entries.every(isDownloaded)) return "downloaded"
  return entries.some((entry) => entry.monitored && !isDownloaded(entry)) ? "missing" : null
}

/**
 * Recent releases lead the shelf, oldest first, so it reads as one timeline
 * into the upcoming dates. Each side takes its share and the other fills any
 * slots it leaves empty.
 */
function upcomingEntries(entries: CalendarEntry[], today: string): UpcomingShelfEntry[] {
  const eligible = entries
    .filter((entry) => entry.kind === "episode" || entry.dateKind !== "air")
    .sort((left, right) => left.date.localeCompare(right.date))
  const premieres = new Set(eligible.filter(isSeasonPremiere).map(seasonDay))
  const groups = new Map<string, CalendarEntry[]>()
  for (const entry of eligible) {
    const past = entry.date < today
    // A cinema run is not a release anyone can have downloaded.
    if (past && entry.kind === "movie" && entry.dateKind === "cinema") continue
    const key = shelfKey(entry, past, premieres)
    const group = groups.get(key)
    if (group) group.push(entry)
    else groups.set(key, [entry])
  }

  const past: UpcomingShelfEntry[] = []
  const future: UpcomingShelfEntry[] = []
  for (const group of groups.values()) {
    const entry = group.find(isSeasonPremiere) ?? group[group.length - 1]
    if (entry.date < today) past.push({ entry, past: true, status: releaseStatus(group) })
    else future.push({ entry, past: false, status: null })
  }
  past.sort((left, right) => right.entry.date.localeCompare(left.entry.date))
  future.sort((left, right) => left.entry.date.localeCompare(right.entry.date))

  const recentTarget = Math.round(UPCOMING_LIMIT * RECENT_SHARE)
  const futureCount = Math.min(future.length, UPCOMING_LIMIT - Math.min(past.length, recentTarget))
  const pastCount = Math.min(past.length, UPCOMING_LIMIT - futureCount)
  return [...past.slice(0, pastCount).reverse(), ...future.slice(0, futureCount)]
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

const STATUS_LABELS: Record<ReleaseStatus, string> = {
  downloaded: "Downloaded",
  missing: "Missing",
}

function UpcomingCard({ entry, past, status }: UpcomingShelfEntry) {
  const location = useLocation()
  const viewing = useViewing()
  const newSeason = isSeasonPremiere(entry)
  const itemId = entry.kind === "episode"
    ? newSeason ? entry.seriesLibraryItemId : entry.libraryItemId ?? entry.seriesLibraryItemId
    : entry.libraryItemId
  // Episodes show their series' poster. A title not in the library yet falls
  // back to the TMDB poster the Companion found for it.
  const posterOwner = entry.kind === "episode" ? entry.seriesLibraryItemId : entry.libraryItemId
  const posters = [
    posterOwner ? imageUrl({ id: posterOwner, primaryImageTag: null }) : null,
    api.collections.providerArtworkUrl(entry.posterPath),
  ].filter((poster): poster is string => poster != null)
  const [imageIndex, setImageIndex] = useState(0)
  const image = posters[imageIndex]
  const title = entry.kind === "episode" ? entry.seriesTitle ?? entry.title : entry.title
  const code = episodeCode(entry)
  // As in the calendar, episode names stay hidden under spoiler protection:
  // whether this user has watched an entry is not known here.
  const episodeName = !newSeason && viewing.data?.spoilerProtection === false ? entry.title : null
  const subtitle = entry.kind === "episode"
    ? [code, episodeName].filter(Boolean).join(" · ") || "Episode"
    : movieReleaseLabel(entry)
  const destination = itemId ? `/item/${encodeURIComponent(itemId)}` : "/calendar"
  const statusLabel = status ? STATUS_LABELS[status] : null
  const StatusIcon = status === "missing" ? CircleAlert : Check
  const linkName = [title, entry.kind === "episode" && !newSeason ? code : null, statusLabel].filter(Boolean).join(", ")

  return (
    <article className="signal-card home-media-card group flex w-poster-w shrink-0 snap-start flex-col gap-2">
      <div className="media-frame relative h-poster-h w-poster-w overflow-hidden rounded-media bg-card ring-1 ring-hairline">
        <Link to={destination} state={itemId ? detailNavigationState(location) : undefined} aria-label={`Open ${linkName}`} className="absolute inset-0 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset">
          {image ? <img src={image} alt="" decoding="async" onError={() => setImageIndex((current) => current + 1)} className="media-artwork-image h-full w-full object-cover" /> : <span className="flex h-full w-full items-center justify-center px-3 text-center text-xs text-muted-foreground">{title}</span>}
          {statusLabel && (
            <span title={statusLabel} className={cn("absolute top-0 left-0 z-[4] grid size-6 place-items-center", status === "missing" ? "bg-destructive text-white" : "bg-primary text-primary-foreground")}>
              <StatusIcon className="size-3.5" aria-hidden />
            </span>
          )}
          <span className={cn("data-label absolute top-0 right-0 z-[4] px-1.5 py-1 leading-none", past ? "bg-background/85 text-foreground" : "bg-primary text-primary-foreground")}>{upcomingDate(entry.date)}</span>
          {newSeason && <span className="data-label absolute inset-x-0 bottom-0 z-[4] bg-primary py-1.5 text-center whitespace-nowrap text-primary-foreground">NEW SEASON</span>}
        </Link>
      </div>
      <Link to={destination} state={itemId ? detailNavigationState(location) : undefined} className="min-w-0 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <div className="truncate text-sm font-medium transition-colors group-hover:text-primary">{title}</div>
        <div className="data-value truncate text-muted-foreground">{subtitle}</div>
      </Link>
    </article>
  )
}

/**
 * Today, drawn in the gap between the last released card and the first
 * upcoming one. The negative margins give back the extra flex gap, so the
 * cards keep their usual spacing and the mark sits centred between them.
 */
function TodayMark() {
  return (
    <div role="separator" aria-orientation="vertical" aria-label="Today" className="-mx-[calc(var(--card-gap)/2)] flex h-poster-h w-0 shrink-0 flex-col items-center gap-1.5">
      <span className="data-label leading-none text-primary [writing-mode:vertical-rl]">Today</span>
      <span className="release-today-rule min-h-0 flex-1" />
    </div>
  )
}

function UpcomingRow({ entries }: { entries: UpcomingShelfEntry[] }) {
  if (!entries.length) return null
  const first = entries[0].entry
  // Only a boundary inside the shelf is marked; at either end it would mark nothing.
  const todayIndex = entries.findIndex((item) => !item.past)
  return (
    <MediaRail title="Release Timeline" viewAll="/calendar" itemCount={entries.length} resetKey={`${first.kind}-${first.date}`}>
      {entries.map(({ entry, past, status }, index) => (
        <Fragment key={`${entry.kind}-${entry.dateKind}-${entry.date}-${entry.tmdbId ?? entry.tvdbId ?? entry.title}-${index}`}>
          {index > 0 && index === todayIndex && <TodayMark />}
          <UpcomingCard entry={entry} past={past} status={status} />
        </Fragment>
      ))}
    </MediaRail>
  )
}

function rowViewAll(row: HomeRow) {
  if (row.kind === "genre") return `/library?kind=Movie,Series&genre=${encodeURIComponent(row.id)}&sort=rating`
  if (row.kind === "collection") return `/collections/mine/${encodeURIComponent(row.id)}`
  switch (row.id) {
    case "recentlyAdded": return "/library?kind=Movie&sort=added"
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
  const upcoming = useMemo(() => upcomingEntries(calendar.data?.entries ?? [], releaseWindow.today), [calendar.data?.entries, releaseWindow.today])

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
      return row ? <Row key={`${element.kind}:${element.id}`} row={row} landscape={row.kind === "builtIn" && row.id === "recentlyAddedShows"} /> : null
    })}
  </div></div>
}
