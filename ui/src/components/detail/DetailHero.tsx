import { Star } from "lucide-react"
import { useState, type ReactNode } from "react"
import { Link } from "react-router-dom"
import { DetailRatingReadout } from "@/components/RatingOverlay"
import { DetailBackLink } from "@/components/detail/DetailPrimitives"
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
import { cn } from "@/lib/utils"
import type { DetailNavigationState } from "@/lib/navigation"

/**
 * Where this item sits: the series and season above an episode, the series
 * above a season. Both are real navigation targets, which is what turns the
 * episode page from a dead end into part of a show.
 */
function Breadcrumb({ item, navigationState }: { item: ItemDetail; navigationState?: DetailNavigationState }) {
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

/**
 * A genre chip browses the library within the item's own kind — `kind` has to
 * be spelled out because `/library` falls back to Movie when the parameter is
 * absent, which would answer a series' genre link with films.
 */
function genreHref(genre: string, kind: string) {
  const browseKind = kind === "Movie" ? "Movie" : "Series"
  return `/library?kind=${browseKind}&genre=${encodeURIComponent(genre)}`
}

/**
 * Community score out of ten, with the critic score alongside once the live
 * `about` record has delivered one — the thin cached row never carries it.
 */
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
        className={cn("media-artwork-image w-full object-cover", isStill ? "aspect-video" : "aspect-2/3")}
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
  about,
  aboutPending = false,
  aboutFailed = false,
  episodeCount,
  navigationState,
  children,
}: {
  item: ItemDetail
  /** The live rich record; the hero paints from the cached row without it. */
  about?: ItemAbout
  aboutPending?: boolean
  aboutFailed?: boolean
  /** Shown instead of a runtime for containers, which have none of their own. */
  episodeCount?: number | null
  navigationState?: DetailNavigationState
  children?: ReactNode
}) {
  const [backdropFailed, setBackdropFailed] = useState(false)
  const [logoFailed, setLogoFailed] = useState(false)
  const backdrop = backdropUrl(item)
  const showBackdrop = Boolean(backdrop) && !backdropFailed
  const logo = logoUrl(item)
  const showLogo = Boolean(logo) && !logoFailed

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
            className="media-backdrop-image h-full w-full object-cover"
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
          {navigationState && (
            <DetailBackLink to={navigationState.from} label={navigationState.label} />
          )}
          <Breadcrumb item={item} navigationState={navigationState} />
          <div className="flex flex-col gap-1">
            {/* The heading stays in the document whatever is drawn: a wordmark
                is artwork, and a page whose <h1> is an image is a page with no
                title. Where the server has one it takes the heading's place
                visually and the text remains for anything reading the page
                rather than looking at it. */}
            <h1 className={cn(showLogo && "sr-only")}>
              <span className="block max-w-4xl text-4xl leading-[0.98] font-black tracking-[-0.04em] text-balance drop-shadow-lg sm:text-5xl lg:text-6xl">
                {item.name}
              </span>
            </h1>
            {showLogo && (
              <img
                src={logo!}
                alt=""
                decoding="async"
                onError={() => setLogoFailed(true)}
                className="max-h-24 w-auto max-w-md self-start object-contain object-left drop-shadow-2xl sm:max-h-28"
              />
            )}
            {item.originalTitle && item.originalTitle !== item.name && (
              <p className="text-sm text-muted-foreground">{item.originalTitle}</p>
            )}
          </div>

          {/* Facts in the data face, divided by accent slashes — the same
              treatment the billboard and the hover card use, so a measured
              value looks the same wherever it appears. */}
          <div className="data-value flex flex-wrap items-center gap-x-2 gap-y-2 text-muted-foreground">
            {facts.map((fact, index) => (
              <span key={fact} className="flex items-center gap-2">
                {index > 0 && (
                  <span className="text-primary/45" aria-hidden>
                    /
                  </span>
                )}
                {fact}
              </span>
            ))}
            {item.officialRating && (
              <Badge variant="outline" className="data-label border-muted-foreground/40 px-1.5 py-0.5">
                {item.officialRating}
              </Badge>
            )}
            <JellyfinRatings item={item} about={about} />
            <DetailRatingReadout item={item} />
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

          {/* The synopsis is live-only. Skeleton lines while it loads keep the
              hero from reflowing under the reader; if the server cannot answer,
              a plain quiet note is the whole degraded state. */}
          {aboutPending ? (
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
          ) : null}

          {children}
        </div>
      </div>
    </header>
  )
}
