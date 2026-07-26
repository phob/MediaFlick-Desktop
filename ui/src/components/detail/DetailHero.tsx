import { Star } from "lucide-react"
import { useState, type ReactNode } from "react"
import { Link } from "react-router-dom"
import { Badge } from "@/components/ui/badge"
import {
  DETAIL_POSTER_WIDTH,
  backdropUrl,
  imageUrl,
  progressFraction,
  type ItemDetail,
} from "@/lib/api"
import { formatRuntime, formatYearOnly } from "@/lib/format"
import { cn } from "@/lib/utils"

/**
 * Where this item sits: the series and season above an episode, the series
 * above a season. Both are real navigation targets, which is what turns the
 * episode page from a dead end into part of a show.
 */
function Breadcrumb({ item }: { item: ItemDetail }) {
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
          <Link to={link.to} className="rounded-sm hover:text-foreground hover:underline">
            {link.label}
          </Link>
        </span>
      ))}
    </nav>
  )
}

/**
 * A genre chip browses the library within the item's own kind — `kind` has to
 * be spelled out because `/library` falls back to Movie when the parameter is
 * absent, which would answer a series' genre link with films.
 */
function genreHref(genre: string, kind: string) {
  const browseKind = kind === "Movie" ? "Movie" : "Series"
  return `/library?kind=${browseKind}&genre=${encodeURIComponent(genre)}`
}

/** Community score out of ten, with the critic score alongside where there is one. */
function Ratings({ item }: { item: ItemDetail }) {
  return (
    <>
      {item.communityRating != null && (
        <span className="flex items-center gap-1 text-foreground">
          <Star className="size-3.5 fill-current text-amber-400" aria-hidden />
          {item.communityRating.toFixed(1)}
        </span>
      )}
      {item.criticRating != null && (
        <span title="Critic score">{Math.round(item.criticRating)}% critics</span>
      )}
    </>
  )
}

function Poster({ item }: { item: ItemDetail }) {
  const [failed, setFailed] = useState(false)
  const progress = progressFraction(item)
  // An episode's primary image is a 16:9 still, not a poster. Cropping it into
  // a 2:3 frame throws away most of the frame for no reason.
  const isStill = item.kind === "Episode"

  if (!item.primaryImageTag || failed) return null
  return (
    <div
      className={cn(
        "relative hidden shrink-0 overflow-hidden rounded-xl shadow-2xl shadow-black/60 ring-1 ring-white/10 sm:block",
        isStill ? "w-[340px]" : "w-[220px]",
      )}
    >
      <img
        src={imageUrl(item, "Primary", DETAIL_POSTER_WIDTH)}
        alt=""
        decoding="async"
        onError={() => setFailed(true)}
        className={cn("w-full object-cover", isStill ? "aspect-video" : "aspect-2/3")}
      />
      {progress > 0 && (
        <div className="absolute inset-x-0 bottom-0 h-1 bg-black/60">
          <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
        </div>
      )}
    </div>
  )
}

/**
 * The top of every detail page: the item's own backdrop bled behind the poster,
 * title, facts, and action bar.
 *
 * The backdrop is a background, not content — it sits under two gradients so
 * text keeps its contrast whatever the artwork looks like, and the whole block
 * falls back to the flat page background when an item has no backdrop at all.
 */
export function DetailHero({
  item,
  episodeCount,
  children,
}: {
  item: ItemDetail
  /** Shown instead of a runtime for containers, which have none of their own. */
  episodeCount?: number | null
  children?: ReactNode
}) {
  const [backdropFailed, setBackdropFailed] = useState(false)
  const backdrop = backdropUrl(item)
  const showBackdrop = Boolean(backdrop) && !backdropFailed

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
  ].filter(Boolean)

  return (
    <header className="relative isolate">
      {showBackdrop && (
        <div className="absolute inset-0 -z-10 overflow-hidden">
          <img
            src={backdrop!}
            alt=""
            decoding="async"
            onError={() => setBackdropFailed(true)}
            className="h-full w-full object-cover object-top"
          />
          {/* Two passes, each fading out well before the other takes over: one
              dissolves the bottom edge into the page, the other keeps the text
              column readable. Overlapping them at full strength is what turns a
              backdrop into a grey rectangle. */}
          <div className="absolute inset-0 bg-linear-to-t from-background from-5% via-background/40 via-45% to-transparent" />
          <div className="absolute inset-0 bg-linear-to-r from-background from-10% via-background/55 via-45% to-transparent to-80%" />
        </div>
      )}

      <div
        className={cn(
          "flex gap-8 px-6 pb-10",
          // Without artwork there is nothing to clear, so the block does not
          // need the height the backdrop asks for.
          showBackdrop ? "min-h-[26rem] pt-14" : "pt-6",
        )}
      >
        <Poster item={item} />
        <div className="flex min-w-0 flex-1 flex-col gap-4">
          <Breadcrumb item={item} />
          <div className="flex flex-col gap-1">
            <h1 className="text-3xl font-semibold tracking-tight text-balance">{item.name}</h1>
            {item.originalTitle && item.originalTitle !== item.name && (
              <p className="text-sm text-muted-foreground">{item.originalTitle}</p>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-x-3 gap-y-2 text-sm text-muted-foreground">
            {facts.map((fact) => (
              <span key={fact}>{fact}</span>
            ))}
            {item.officialRating && (
              <Badge variant="outline" className="border-muted-foreground/40">
                {item.officialRating}
              </Badge>
            )}
            <Ratings item={item} />
          </div>

          {item.genres.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {item.genres.map((genre) => (
                <Badge
                  key={genre}
                  variant="secondary"
                  asChild
                  className="hover:bg-secondary/70"
                >
                  <Link to={genreHref(genre, item.kind)}>{genre}</Link>
                </Badge>
              ))}
            </div>
          )}

          {item.overview && (
            <p className="max-w-3xl text-sm leading-relaxed text-foreground/90">{item.overview}</p>
          )}

          {children}
        </div>
      </div>
    </header>
  )
}
