import {
  Check,
  ChevronLeft,
  ChevronRight,
  Film,
  Info,
  Play,
  Plus,
  Tv,
} from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { Link } from "react-router-dom"
import { MediaCard } from "@/components/MediaCard"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  BACKDROP_WIDTH,
  landscapeImageCandidates,
  progressFraction,
  type HomeRow,
  type ItemDetail,
  type ItemSummary,
} from "@/lib/api"
import { formatRemaining, formatRuntime } from "@/lib/format"
import {
  useHome,
  useItem,
  useItems,
  useNextUp,
  usePlay,
  useSetFavorite,
} from "@/lib/queries"
import { cn } from "@/lib/utils"

const VIEW_ALL: Partial<Record<HomeRow["id"], string>> = {
  recent: "/library?sort=added",
  favorites: "/library?favorite=true",
}

function episodeLabel(item: ItemSummary) {
  if (item.kind !== "Episode") return null
  const season = item.parentIndexNumber
  const episode = item.indexNumber
  const code = season != null && episode != null ? `S${season} E${episode}` : null
  return [code, item.name].filter(Boolean).join(" · ")
}

function HeroBackdrop({ item }: { item: ItemSummary }) {
  const images = landscapeImageCandidates(item, BACKDROP_WIDTH)
  const [imageIndex, setImageIndex] = useState(0)
  const image = images[imageIndex]

  if (!image) return null
  return (
    <div className="pointer-events-none absolute inset-0">
      <img
        src={image}
        alt=""
        decoding="async"
        onError={() => setImageIndex((current) => current + 1)}
        className="h-full w-full object-cover object-center"
      />
      <div className="absolute inset-0 bg-linear-to-r from-background from-5% via-background/78 via-45% to-background/10" />
      <div className="absolute inset-0 bg-linear-to-t from-background via-transparent via-55% to-background/15" />
    </div>
  )
}

function FeaturedHero({
  item,
  detail,
  source,
  playTarget,
}: {
  item: ItemSummary
  detail?: ItemDetail
  source: "resume" | "recent"
  playTarget?: ItemSummary | null
}) {
  const play = usePlay()
  const setFavorite = useSetFavorite()
  const current = detail ?? item
  const progress = progressFraction(current)
  const remaining = formatRemaining(current.positionTicks, current.runtimeTicks)
  const isEpisode = current.kind === "Episode"
  const target = current.kind === "Series" ? playTarget : current
  const isPlayable = Boolean(target && (target.kind === "Movie" || target.kind === "Episode"))
  const title = isEpisode && current.seriesName ? current.seriesName : current.name
  const secondaryTitle = episodeLabel(current)
  const kicker =
    source === "resume"
      ? current.positionTicks > 0
        ? "Continue watching"
        : "Next up"
      : "Recently added"
  const facts = [
    current.year ? String(current.year) : null,
    formatRuntime(current.runtimeTicks),
    current.officialRating,
    current.communityRating != null ? `★ ${current.communityRating.toFixed(1)}` : null,
  ].filter(Boolean)

  return (
    <section
      className="relative flex min-h-[28rem] items-end overflow-hidden px-6 pb-24 sm:min-h-[32rem] sm:px-10 sm:pb-28 lg:min-h-[36rem] lg:px-14"
      aria-labelledby="featured-title"
    >
      <HeroBackdrop key={current.id} item={current} />
      <div className="relative z-10 flex max-w-2xl flex-col items-start gap-4">
        <p className="flex items-center gap-2 text-xs font-semibold tracking-[0.18em] text-foreground/75 uppercase">
          <span className="size-1.5 rounded-full bg-primary" aria-hidden />
          {kicker}
        </p>

        <div className="flex flex-col gap-2">
          <h1
            id="featured-title"
            className="max-w-xl text-4xl leading-[0.96] font-black tracking-[-0.04em] text-balance drop-shadow-lg sm:text-5xl lg:text-6xl"
          >
            {title}
          </h1>
          {secondaryTitle && (
            <p className="text-base font-medium text-foreground/90 sm:text-lg">{secondaryTitle}</p>
          )}
        </div>

        {facts.length > 0 && (
          <div className="flex flex-wrap items-center gap-2 text-sm font-medium text-foreground/80">
            {facts.map((fact, index) => (
              <span key={`${fact}-${index}`} className="flex items-center gap-2">
                {index > 0 && <span className="size-1 rounded-full bg-foreground/35" aria-hidden />}
                {fact}
              </span>
            ))}
          </div>
        )}

        {detail?.overview && (
          <p className="line-clamp-3 max-w-xl text-sm leading-relaxed text-foreground/85 drop-shadow sm:text-base">
            {detail.overview}
          </p>
        )}

        {progress > 0 && (
          <div className="flex w-full max-w-md items-center gap-3">
            <div className="h-1 flex-1 overflow-hidden rounded-full bg-white/25">
              <div className="h-full rounded-full bg-primary" style={{ width: `${progress * 100}%` }} />
            </div>
            {remaining && <span className="text-xs font-medium text-foreground/75">{remaining}</span>}
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2 pt-1">
          {isPlayable && (
            <Button
              size="lg"
              disabled={play.isPending}
              className="min-w-28 bg-white text-black shadow-lg hover:bg-white/85"
              onClick={() =>
                play.mutate({
                  id: target!.id,
                  resume: target!.positionTicks > 0,
                })
              }
            >
              <Play className="size-5 fill-current" />
              {target!.positionTicks > 0 ? "Resume" : "Play"}
            </Button>
          )}
          <Button variant="secondary" size="lg" className="bg-white/20 shadow-lg hover:bg-white/30" asChild>
            <Link to={`/item/${encodeURIComponent(current.id)}`}>
              <Info className="size-5" />
              More info
            </Link>
          </Button>
          <Button
            variant="secondary"
            size="icon-lg"
            className="bg-white/20 shadow-lg hover:bg-white/30"
            aria-label={current.favorite ? "Remove from My List" : "Add to My List"}
            aria-pressed={current.favorite}
            disabled={setFavorite.isPending}
            onClick={() =>
              setFavorite.mutate({
                id: current.id,
                favorite: !current.favorite,
              })
            }
          >
            {current.favorite ? <Check className="size-5" /> : <Plus className="size-5" />}
          </Button>
        </div>
      </div>
    </section>
  )
}

function Row({ row }: { row: HomeRow }) {
  const rail = useRef<HTMLDivElement>(null)
  const [edges, setEdges] = useState({ start: true, end: true })
  const viewAll = VIEW_ALL[row.id]

  const measure = useCallback(() => {
    const node = rail.current
    if (!node) return
    setEdges({
      start: node.scrollLeft <= 2,
      end: node.scrollLeft + node.clientWidth >= node.scrollWidth - 2,
    })
  }, [])

  useEffect(() => {
    const node = rail.current
    if (!node) return
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(node)
    return () => observer.disconnect()
  }, [measure, row.items.length])

  const move = (direction: -1 | 1) => {
    const node = rail.current
    if (!node) return
    node.scrollBy({ left: direction * node.clientWidth * 0.85, behavior: "smooth" })
  }

  if (!row.items.length) return null
  return (
    <section className="group/row flex flex-col gap-3" aria-labelledby={`row-${row.id}`}>
      <div className="flex items-end justify-between gap-4 px-6 sm:px-10 lg:px-14">
        <h2 id={`row-${row.id}`} className="text-lg font-semibold tracking-tight sm:text-xl">
          {row.title}
        </h2>
        {viewAll && (
          <Link
            to={viewAll}
            className="flex items-center gap-0.5 rounded-sm text-xs font-medium text-muted-foreground opacity-70 transition hover:text-foreground hover:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring sm:text-sm"
          >
            Browse all
            <ChevronRight className="size-4" />
          </Link>
        )}
      </div>

      <div className="relative">
        <div
          ref={rail}
          role="region"
          aria-label={`${row.title} titles`}
          tabIndex={0}
          onScroll={measure}
          onKeyDown={(event) => {
            if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
              event.preventDefault()
              move(event.key === "ArrowLeft" ? -1 : 1)
            }
          }}
          className="home-media-rail flex snap-x snap-mandatory gap-[var(--card-gap)] overflow-x-auto px-6 pt-1 pb-4 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset sm:px-10 lg:px-14"
        >
          {row.items.map((item) => (
            <MediaCard
              key={item.id}
              item={item}
              landscape={row.id === "resume"}
              className="home-media-card shrink-0 snap-start"
            />
          ))}
        </div>

        <button
          type="button"
          aria-label={`Previous ${row.title} titles`}
          disabled={edges.start}
          onClick={() => move(-1)}
          className={cn(
            "absolute top-1/2 left-1 z-20 flex h-20 w-9 -translate-y-1/2 items-center justify-center rounded-md bg-black/70 text-white shadow-xl backdrop-blur-sm transition hover:bg-black/90 focus-visible:ring-2 focus-visible:ring-white disabled:pointer-events-none sm:left-2",
            edges.start ? "opacity-0" : "opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100",
          )}
        >
          <ChevronLeft className="size-7" />
        </button>
        <button
          type="button"
          aria-label={`Next ${row.title} titles`}
          disabled={edges.end}
          onClick={() => move(1)}
          className={cn(
            "absolute top-1/2 right-1 z-20 flex h-20 w-9 -translate-y-1/2 items-center justify-center rounded-md bg-black/70 text-white shadow-xl backdrop-blur-sm transition hover:bg-black/90 focus-visible:ring-2 focus-visible:ring-white disabled:pointer-events-none sm:right-2",
            edges.end ? "opacity-0" : "opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100",
          )}
        >
          <ChevronRight className="size-7" />
        </button>
      </div>
    </section>
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
    <div className="flex flex-col gap-9 pb-12">
      <div className="relative flex min-h-[32rem] items-end px-10 pb-28">
        <Skeleton className="absolute inset-0 rounded-none" />
        <div className="relative z-10 flex w-full max-w-xl flex-col gap-4">
          <Skeleton className="h-3 w-32" />
          <Skeleton className="h-14 w-3/4" />
          <Skeleton className="h-4 w-52" />
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-10 w-64" />
        </div>
      </div>
      <div className="-mt-20 flex flex-col gap-9">
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

export default function Home() {
  const home = useHome()
  const favorites = useItems({ favorite: true, sort: "added", limit: 24 })
  const rows = home.data?.rows.filter((row) => row.items.length > 0) ?? []
  const resume = rows.find((row) => row.id === "resume")
  const recent = rows.find((row) => row.id === "recent")
  const featured =
    resume?.items[0] ??
    recent?.items.find((item) => item.kind === "Movie") ??
    recent?.items[0]
  const featuredDetail = useItem(featured?.id)
  const featuredNextUp = useNextUp(featured?.id, featured?.kind === "Series")

  if (home.error) {
    return <p className="p-6 text-sm text-destructive">{home.error.message}</p>
  }

  if (home.isPending) return <HomeSkeleton />

  const favoriteRow: HomeRow | null = favorites.data?.items.length
    ? { id: "favorites", title: "My List", items: favorites.data.items }
    : null
  const displayRows = favoriteRow ? [...rows, favoriteRow] : rows

  if (!featured && !displayRows.length) return <EmptyHome />

  return (
    <div className="home-page flex min-h-full flex-col pb-12">
      {featured && (
        <FeaturedHero
          item={featured}
          detail={featuredDetail.data}
          source={resume?.items[0]?.id === featured.id ? "resume" : "recent"}
          playTarget={featuredNextUp.data?.item}
        />
      )}
      <div className={cn("relative z-10 flex flex-col gap-9", featured && "-mt-20")}>
        {displayRows.map((row) => (
          <Row key={row.id} row={row} />
        ))}
      </div>
    </div>
  )
}
