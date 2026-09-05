import { useViewing } from "@/lib/viewing"
import {
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Clapperboard,
  Play,
  Ticket,
} from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { PageEmptyState, PageErrorState, PageHeader } from "@/components/PageHeader"
import { RequestDialog } from "@/components/seerr/RequestDialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  type CalendarDateKind,
  type CalendarEntry,
  type CalendarSource,
  type SeerrResult,
} from "@/lib/api"
import { usePlay, useReleaseCalendar, useSeerrStatus } from "@/lib/queries"
import { detailNavigationState } from "@/lib/navigation"
import { cn } from "@/lib/utils"

const DATE_KINDS: { id: CalendarDateKind; label: string }[] = [
  { id: "air", label: "Episodes" },
  { id: "digital", label: "Digital" },
  { id: "cinema", label: "Cinema" },
  { id: "physical", label: "Physical" },
]
const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

function iso(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, "0")
  const day = String(date.getDate()).padStart(2, "0")
  return `${year}-${month}-${day}`
}

function addMonths(date: Date, amount: number) {
  return new Date(date.getFullYear(), date.getMonth() + amount, 1)
}

function calendarWindow(month: Date) {
  const gridStart = new Date(month.getFullYear(), month.getMonth(), 1)
  const mondayOffset = (gridStart.getDay() + 6) % 7
  gridStart.setDate(gridStart.getDate() - mondayOffset)
  const gridEnd = new Date(gridStart)
  gridEnd.setDate(gridStart.getDate() + 41)
  return { start: iso(gridStart), end: iso(gridEnd), gridStart }
}

function episodeLabel(entry: CalendarEntry) {
  if (entry.kind !== "episode") return null
  if (entry.season == null || entry.episode == null) return entry.seriesTitle
  return `${entry.seriesTitle ?? "Episode"} · S${String(entry.season).padStart(2, "0")}E${String(entry.episode).padStart(2, "0")}`
}

function entriesByDate(entries: CalendarEntry[]) {
  const groups = new Map<string, CalendarEntry[]>()
  for (const entry of entries) {
    const group = groups.get(entry.date)
    if (group) group.push(entry)
    else groups.set(entry.date, [entry])
  }
  return groups
}

function EntryActions({ entry }: { entry: CalendarEntry }) {
  const location = useLocation()
  const navigationState = detailNavigationState(location)
  const play = usePlay()
  const seerr = useSeerrStatus()
  const [requesting, setRequesting] = useState(false)

  if (entry.libraryItemId) {
    return (
      <div className="flex items-center gap-1">
        <Button
          size="icon-sm"
          variant="ghost"
          aria-label={`Play ${entry.title}`}
          disabled={play.isPending}
          onClick={() => play.mutate({ id: entry.libraryItemId!, resume: false })}
        >
          <Play className="size-3.5" />
        </Button>
        <Button asChild size="sm" variant="ghost">
          <Link to={`/item/${encodeURIComponent(entry.libraryItemId)}`} state={navigationState}>Details</Link>
        </Button>
      </div>
    )
  }

  if (entry.seriesLibraryItemId) {
    return (
      <Button asChild size="sm" variant="ghost">
        <Link to={`/item/${encodeURIComponent(entry.seriesLibraryItemId)}`} state={navigationState}>Series details</Link>
      </Button>
    )
  }

  const requestMediaType = entry.kind === "movie" ? "movie" : "tv"
  const requestTmdbId = entry.kind === "movie" ? entry.tmdbId : entry.seriesTmdbId
  if (requestTmdbId && seerr.data?.linked) {
    const result: SeerrResult = {
      mediaType: requestMediaType,
      tmdbId: requestTmdbId,
      title: entry.kind === "episode" ? entry.seriesTitle ?? entry.title : entry.title,
      year: Number(entry.date.slice(0, 4)) || null,
      overview: null,
      posterPath: null,
      backdropPath: null,
      voteAverage: null,
      status: "unknown",
      status4k: "unknown",
      libraryItemId: null,
    }
    return (
      <>
        <Button size="sm" variant="ghost" onClick={() => setRequesting(true)}>
          <Ticket className="size-3.5" />
          Request options
        </Button>
        {requesting && <RequestDialog result={result} onClose={() => setRequesting(false)} />}
      </>
    )
  }
  return null
}

function Entry({ entry: originalEntry, compact = false }: { entry: CalendarEntry; compact?: boolean }) {
  const viewing = useViewing()
  const entry = originalEntry.kind === "episode" && viewing.data?.spoilerProtection !== false ? {...originalEntry, title: originalEntry.seriesTitle ?? "Upcoming episode"} : originalEntry
  return (
    <article
      className={cn(
        "group flex min-w-0 items-center gap-3 border border-white/6 bg-card/55",
        compact ? "rounded-media px-2 py-1.5" : "rounded-media px-4 py-3",
      )}
    >
      <div
        className={cn(
          "grid shrink-0 place-items-center rounded-media bg-primary/12 text-primary",
          compact ? "size-7" : "size-10",
        )}
      >
        {entry.kind === "movie" ? (
          <Clapperboard className="size-4" />
        ) : (
          <CalendarDays className="size-4" />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{entry.title}</div>
        <div className="truncate font-mono text-[0.66rem] tracking-wide text-muted-foreground uppercase">
          {episodeLabel(entry) ?? entry.dateKind}
        </div>
      </div>
      {!compact && (
        <>
          <Badge variant="outline">{entry.hasFile ? "Ready" : entry.monitored ? "Tracked" : "Untracked"}</Badge>
          <EntryActions entry={entry} />
        </>
      )}
    </article>
  )
}

function Agenda({ entries, today }: { entries: CalendarEntry[]; today: string }) {
  const groups = [...entriesByDate(entries)].sort(([left], [right]) => left.localeCompare(right))
  const todayIndex = groups.findIndex(([date]) => date >= today)
  return (
    <div className="flex max-w-6xl flex-col gap-7">
      {groups.map(([date, dayEntries], index) => (
        <div key={date}>
          {index === todayIndex && <div data-calendar-date={today} className="scroll-mt-4" />}
          <section className="grid gap-3 md:grid-cols-[9rem_1fr]">
            <div>
              <div className="font-mono text-sm font-semibold tracking-wide text-primary">
                {new Date(`${date}T12:00:00`).toLocaleDateString(undefined, {
                  weekday: "short",
                  month: "short",
                  day: "numeric",
                })}
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                {dayEntries.length} {dayEntries.length === 1 ? "release" : "releases"}
              </div>
            </div>
            <div className="flex flex-col gap-2">
              {dayEntries.map((entry, entryIndex) => (
                <Entry
                  key={`${entry.kind}-${entry.tmdbId}-${entry.tvdbId}-${entry.dateKind}-${entryIndex}`}
                  entry={entry}
                />
              ))}
            </div>
          </section>
        </div>
      ))}
      {todayIndex === -1 && <div data-calendar-date={today} className="scroll-mt-4" />}
    </div>
  )
}

function MonthGrid({
  month,
  gridStart,
  entries,
}: {
  month: Date
  gridStart: Date
  entries: CalendarEntry[]
}) {
  const byDate = entriesByDate(entries)
  const days = Array.from({ length: 42 }, (_, index) => {
    const day = new Date(gridStart)
    day.setDate(gridStart.getDate() + index)
    return day
  })
  return (
    <div className="overflow-hidden rounded-media border border-white/7 bg-card/30">
      <div className="grid grid-cols-7 border-b border-white/7 bg-white/3">
        {WEEKDAYS.map((weekday) => (
          <div key={weekday} className="px-2 py-2 font-mono text-[0.65rem] tracking-widest text-muted-foreground uppercase">
            {weekday}
          </div>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {days.map((day) => {
          const date = iso(day)
          const items = byDate.get(date) ?? []
          const outside = day.getMonth() !== month.getMonth()
          return (
            <div
              key={date}
              data-calendar-date={date}
              className={cn(
                "min-h-32 scroll-mt-4 border-r border-b border-white/6 p-2 last:border-r-0",
                outside && "bg-black/12 text-muted-foreground/50",
              )}
            >
              <div className={cn("mb-2 font-mono text-xs", iso(new Date()) === date && "text-primary")}>
                {day.getDate()}
              </div>
              <div className="flex flex-col gap-1">
                {items.slice(0, 3).map((entry, index) => (
                  <Entry key={`${entry.kind}-${entry.tmdbId}-${entry.dateKind}-${index}`} entry={entry} compact />
                ))}
                {items.length > 3 && (
                  <span className="px-1 font-mono text-[0.65rem] text-muted-foreground">
                    +{items.length - 3} more
                  </span>
                )}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function SourceWarning({ sources }: { sources: Record<string, CalendarSource> }) {
  const stale = Object.entries(sources).filter(([, source]) => source.stale || (source.enabled && !source.available))
  if (!stale.length) return null
  return (
    <div className="flex items-start gap-2 rounded-media border border-amber-400/20 bg-amber-400/7 px-3 py-2 text-sm text-amber-100">
      <CircleAlert className="mt-0.5 size-4 shrink-0" />
      <span>
        {stale.map(([name]) => name).join(" and ")} unavailable. Showing the last cached releases.
      </span>
    </div>
  )
}

export default function Calendar() {
  const [month, setMonth] = useState(() => new Date(new Date().getFullYear(), new Date().getMonth(), 1))
  const [view, setView] = useState("agenda")
  const [kinds, setKinds] = useState<Set<CalendarDateKind>>(() => new Set(DATE_KINDS.map((kind) => kind.id)))
  const [todayJumpRequest, setTodayJumpRequest] = useState(1)
  const completedTodayJump = useRef(0)
  const calendarRoot = useRef<HTMLDivElement>(null)
  const today = iso(new Date())
  const window = useMemo(() => calendarWindow(month), [month])
  const calendar = useReleaseCalendar(window.start, window.end)
  const entries = useMemo(
    () => (calendar.data?.entries ?? []).filter((entry) => kinds.has(entry.dateKind)),
    [calendar.data?.entries, kinds],
  )

  useEffect(() => {
    if (!calendar.data || completedTodayJump.current === todayJumpRequest) return
    const target = calendarRoot.current?.querySelector<HTMLElement>(`[data-calendar-date="${today}"]`)
    if (!target) return
    target.scrollIntoView({
      behavior: todayJumpRequest === 1 ? "auto" : "smooth",
      block: "start",
    })
    completedTodayJump.current = todayJumpRequest
  }, [calendar.data, entries, today, todayJumpRequest, view])

  const jumpToToday = () => {
    const now = new Date()
    setMonth(new Date(now.getFullYear(), now.getMonth(), 1))
    setTodayJumpRequest((request) => request + 1)
  }

  return (
    <div ref={calendarRoot} className="flex min-h-full min-w-0 flex-col">
      <PageHeader
        eyebrow="Release calendar"
        title={month.toLocaleDateString(undefined, { month: "long", year: "numeric" })}
        contentClassName={view === "agenda" ? "max-w-6xl" : undefined}
        actions={
          <div className="flex items-center gap-2">
            <Button aria-label="Previous month" variant="outline" size="icon" onClick={() => setMonth(addMonths(month, -1))}>
              <ChevronLeft />
            </Button>
            <Button variant="outline" onClick={jumpToToday}>
              Today
            </Button>
            <Button aria-label="Next month" variant="outline" size="icon" onClick={() => setMonth(addMonths(month, 1))}>
              <ChevronRight />
            </Button>
          </div>
        }
      />

      <div className="flex min-w-0 flex-col gap-5 px-6 pb-10 sm:px-10 lg:px-14">
        <div className={cn("flex flex-wrap items-center justify-between gap-3", view === "agenda" && "max-w-6xl")}>
          <div className="flex flex-wrap gap-2">
            {DATE_KINDS.map((kind) => (
              <Button
                key={kind.id}
                size="sm"
                variant={kinds.has(kind.id) ? "secondary" : "ghost"}
                onClick={() =>
                  setKinds((current) => {
                    const next = new Set(current)
                    if (next.has(kind.id)) next.delete(kind.id)
                    else next.add(kind.id)
                    return next
                  })
                }
              >
                {kind.label}
              </Button>
            ))}
          </div>
          <Tabs value={view} onValueChange={setView}>
            <TabsList>
              <TabsTrigger value="agenda">Agenda</TabsTrigger>
              <TabsTrigger value="month">Month</TabsTrigger>
            </TabsList>
          </Tabs>
        </div>

        {calendar.data && (
          <div className={cn(view === "agenda" && "max-w-6xl")}>
            <SourceWarning sources={calendar.data.sources} />
          </div>
        )}
        {calendar.error && !calendar.data ? (
          <div className={cn(view === "agenda" && "max-w-6xl")}>
            <PageErrorState
              title="Could not load releases"
              description={calendar.error.message}
              action={<Button variant="outline" onClick={() => void calendar.refetch()}>Try again</Button>}
            />
          </div>
        ) : calendar.isPending ? (
          <div className={cn("flex flex-col gap-3", view === "agenda" && "max-w-6xl")}>
            {Array.from({ length: 6 }, (_, index) => <Skeleton key={index} className="h-20" />)}
          </div>
        ) : entries.length ? (
          view === "month" ? (
            <MonthGrid month={month} gridStart={window.gridStart} entries={entries} />
          ) : (
            <Agenda entries={entries} today={today} />
          )
        ) : (
          <div data-calendar-date={today} className={cn("scroll-mt-4", view === "agenda" && "max-w-6xl")}>
            <PageEmptyState
              icon={<CalendarDays className="size-6" />}
              title="No releases in this window"
              description="Try another month or enable more release types."
            />
          </div>
        )}
      </div>
    </div>
  )
}
