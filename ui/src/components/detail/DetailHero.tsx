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
        // `self-start`: the hero row is as tall as the backdrop, and without it
        // the frame — ring, shadow and all — stretches into an empty box far
        // below the artwork it holds.
        "relative hidden shrink-0 self-start overflow-hidden rounded-xl shadow-2xl shadow-black/60 ring-1 ring-white/10 sm:block",
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
 * The backdrop is a background and nothing else. It is taken out of flow at its
 * own 16:9 size, so it is never cropped to a hero height and never pushes the
 * hero, cast, seasons, or details around; it simply reaches past this header and
 * those sections draw over it. Only two gradients touch it: one keeps the hero's
 * own text legible, the other dims the artwork away where the page's content
 * begins.
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
    <header className="relative">
      {showBackdrop && (
        // `aspect-video` gives the layer the picture's own height, so the whole
        // frame is there. It deliberately reaches below this header, and the
        // negative z-index puts it behind the whole detail page rather than just
        // behind the hero — which is why this header must not `isolate`, and why
        // the page it sits in does (see `ItemDetail`): without that stacking
        // context the artwork would either cover the sections below or disappear
        // behind the app shell's own background.
        <div className="pointer-events-none absolute inset-x-0 top-0 -z-10 aspect-video">
          <img
            src={backdrop!}
            alt=""
            decoding="async"
            onError={() => setBackdropFailed(true)}
            className="h-full w-full object-cover"
          />
          {/* Two passes over the part of the picture that lies behind the page's
              own content. The first drops it back to a steady wash just past the
              hero, in the few rem before the cast row starts, because names and
              table rows have to stay readable over whatever the artwork happens to
              be doing there. Its stops are in `rem` because they answer to the
              hero's own `min-h-[26rem]` floor, which is where that content begins
              at any window width.

              The second takes that wash to nothing by the picture's last line, so
              it ends in the page colour instead of on a visible edge — and that
              one is a percentage, because the line it has to land on is the
              backdrop's own bottom and this layer is sized `aspect-video`, so its
              height follows the window width. A fixed stop only completed the fade
              on windows wide enough to make the picture that tall; on anything
              narrower the artwork was cut off mid-fade, which is the edge this
              gradient exists to remove. */}
          <div className="absolute inset-0 bg-linear-to-b from-transparent from-[26rem] to-background/65 to-[31rem]" />
          <div className="absolute inset-0 bg-linear-to-b from-transparent from-[80%] to-background" />
        </div>
      )}

      <div
        className={cn(
          "relative flex gap-8 px-6 pb-12 sm:px-10 lg:px-14",
          // Unchanged by the backdrop, which is what keeps everything below this
          // header exactly where it was.
          showBackdrop ? "min-h-[26rem] pt-14" : "pt-6",
        )}
      >
        {/* Contrast for the text, and only for the text: the scrim belongs to this
            block, so it covers what has to stay legible and leaves the rest of the
            picture alone — running it over the whole backdrop is what used to turn
            the lower half into a grey rectangle.

            Both mask axes are needed. Downwards it feathers out, because a flat
            scrim would end on a visible line across the artwork. Sideways it is
            measured in `rem`, not per cent: the text stops at a real width
            (`max-w-3xl` past the poster), so a proportional taper would leave the
            end of a synopsis unprotected on a wide window and dim half the picture
            on a narrow one. */}
        {showBackdrop && (
          <div className="pointer-events-none absolute inset-0 -z-1 bg-background/80 [mask-composite:intersect] [mask-image:linear-gradient(to_right,#000_66rem,transparent_82rem),linear-gradient(to_bottom,#000_55%,transparent)]" />
        )}
        <Poster item={item} />
        <div className="flex min-w-0 flex-1 flex-col gap-4">
          <Breadcrumb item={item} />
          <div className="flex flex-col gap-1">
            <h1 className="max-w-4xl text-4xl leading-[0.98] font-black tracking-[-0.04em] text-balance drop-shadow-lg sm:text-5xl lg:text-6xl">
              {item.name}
            </h1>
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
