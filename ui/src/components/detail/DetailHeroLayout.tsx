import { Fragment, useState, type ReactNode } from "react"
import { Link, type To } from "react-router-dom"
import { DetailBackLink } from "@/components/detail/DetailPrimitives"
import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"

interface HeroArtwork {
  src: string
  aspect?: "poster" | "still"
  progress?: number
}

interface HeroGenre {
  label: string
  to?: To
}

/**
 * The shared visual contract for both library and discovery details.
 *
 * Page-specific components supply data and actions, but the back action,
 * artwork, title, facts, genres, synopsis, spacing, and backdrop treatment all
 * live here. Keeping that dominant structure in one component prevents the two
 * routes from drifting into lookalike-but-different pages again.
 */
export function DetailHeroLayout({
  back,
  backdrop,
  poster,
  logo,
  title,
  subtitle,
  breadcrumb,
  facts,
  metadata,
  genres = [],
  status,
  overview,
  children,
}: {
  back: { to: To; label: string }
  backdrop?: string | null
  poster?: HeroArtwork | null
  logo?: string | null
  title: string
  subtitle?: ReactNode
  breadcrumb?: ReactNode
  facts: ReactNode[]
  metadata?: ReactNode
  genres?: HeroGenre[]
  status?: ReactNode
  overview?: ReactNode
  children?: ReactNode
}) {
  const [failedBackdrop, setFailedBackdrop] = useState<string | null>(null)
  const [failedPoster, setFailedPoster] = useState<string | null>(null)
  const [failedLogo, setFailedLogo] = useState<string | null>(null)
  const showBackdrop = Boolean(backdrop) && failedBackdrop !== backdrop
  const showPoster = Boolean(poster?.src) && failedPoster !== poster?.src
  const showLogo = Boolean(logo) && failedLogo !== logo
  const progress = Math.max(0, Math.min(1, poster?.progress ?? 0))

  return (
    <header className="relative" data-testid="detail-hero-layout">
      {showBackdrop && (
        <div className="pointer-events-none absolute inset-x-0 top-0 -z-10 aspect-video">
          <img
            src={backdrop!}
            alt=""
            decoding="async"
            onError={() => setFailedBackdrop(backdrop!)}
            className="media-backdrop-image h-full w-full object-cover"
          />
          <div className="absolute inset-0 bg-linear-to-b from-transparent from-[26rem] to-background/65 to-[31rem]" />
          <div className="absolute inset-0 bg-linear-to-b from-transparent from-[80%] to-background" />
        </div>
      )}

      <div className="relative flex min-h-[26rem] gap-8 px-6 pt-14 pb-12 sm:px-10 lg:px-14">
        {showBackdrop && (
          <div className="pointer-events-none absolute inset-0 -z-1 bg-background/80 [mask-composite:intersect] [mask-image:linear-gradient(to_right,#000_66rem,transparent_82rem),linear-gradient(to_bottom,#000_55%,transparent)]" />
        )}

        {showPoster && (
          <div
            className={cn(
              "relative hidden shrink-0 self-start overflow-hidden rounded-xl shadow-2xl shadow-black/60 ring-1 ring-white/10 sm:block",
              poster?.aspect === "still" ? "w-[340px]" : "w-[220px]",
            )}
          >
            <img
              src={poster!.src}
              alt=""
              decoding="async"
              onError={() => setFailedPoster(poster!.src)}
              className={cn(
                "media-artwork-image w-full object-cover",
                poster?.aspect === "still" ? "aspect-video" : "aspect-2/3",
              )}
            />
            {progress > 0 && (
              <div className="absolute inset-x-0 bottom-0 h-1 bg-black/60">
                <div className="h-full bg-primary" style={{ width: `${progress * 100}%` }} />
              </div>
            )}
          </div>
        )}

        <div className="flex min-w-0 flex-1 flex-col gap-4">
          <DetailBackLink to={back.to} label={back.label} />
          {breadcrumb}

          <div className="flex flex-col gap-1">
            <h1 className={cn(showLogo && "sr-only")}>
              <span className="block max-w-4xl text-4xl leading-[0.98] font-black tracking-[-0.04em] text-balance drop-shadow-lg sm:text-5xl lg:text-6xl">
                {title}
              </span>
            </h1>
            {showLogo && (
              <img
                src={logo!}
                alt=""
                decoding="async"
                onError={() => setFailedLogo(logo!)}
                className="max-h-24 w-auto max-w-md self-start object-contain object-left drop-shadow-2xl sm:max-h-28"
              />
            )}
            {subtitle}
          </div>

          <div className="data-value flex flex-wrap items-center gap-x-2 gap-y-2 text-muted-foreground">
            {facts.map((fact, index) => (
              <Fragment key={index}>
                {index > 0 && (
                  <span className="text-primary/45" aria-hidden>
                    /
                  </span>
                )}
                <span>{fact}</span>
              </Fragment>
            ))}
            {metadata}
          </div>

          {(genres.length > 0 || status) && (
            <div className="flex flex-wrap items-center gap-2">
              {genres.map((genre) => (
                <Badge
                  key={genre.label}
                  variant="secondary"
                  asChild={Boolean(genre.to)}
                  className={cn(genre.to && "hover:bg-secondary/70")}
                >
                  {genre.to ? <Link to={genre.to}>{genre.label}</Link> : genre.label}
                </Badge>
              ))}
              {status}
            </div>
          )}

          {overview}
          {children}
        </div>
      </div>
    </header>
  )
}
