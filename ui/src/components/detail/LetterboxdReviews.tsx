import { ExternalLink, Star } from "lucide-react"
import { useState } from "react"
import { Skeleton } from "@/components/ui/skeleton"
import type { ItemDetail, LetterboxdReview } from "@/lib/api"
import { useLetterboxdReviews } from "@/lib/queries"
import { cn } from "@/lib/utils"

function ratingLabel(value: number) {
  return Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1)
}

function ReviewCard({ entry }: { entry: LetterboxdReview }) {
  const [expanded, setExpanded] = useState(false)
  const canExpand = Boolean(entry.review && entry.review.length > 420)
  const destination = entry.entryUrl ?? entry.profileUrl

  return (
    <article className="flex min-w-0 flex-col gap-3 border border-border/80 bg-card/75 p-4 shadow-lg shadow-black/10 backdrop-blur-sm">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h3 className="truncate text-sm font-semibold text-foreground">{entry.displayName}</h3>
          <p className="data-value mt-0.5 text-[0.65rem] text-muted-foreground">
            @{entry.username}
            {entry.watchedDate ? ` / ${entry.watchedDate}` : ""}
            {entry.stale ? " / cached" : ""}
          </p>
        </div>
        {entry.rating != null && (
          <span
            className="data-value flex items-center gap-1 text-sm text-foreground"
            aria-label={`${ratingLabel(entry.rating)} out of 5 stars from ${entry.displayName}`}
          >
            <Star className="size-3.5 fill-current text-[#00e054]" aria-hidden />
            {ratingLabel(entry.rating)}
            <span className="text-muted-foreground">/ 5</span>
          </span>
        )}
      </header>

      {entry.review && (
        <>
          <blockquote
            className={cn(
              "whitespace-pre-line text-sm leading-relaxed text-foreground/90",
              !expanded && canExpand && "line-clamp-5",
            )}
          >
            {entry.review}
          </blockquote>
          {canExpand && (
            <button
              type="button"
              className="self-start text-xs font-medium text-primary hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              onClick={() => setExpanded((value) => !value)}
              aria-expanded={expanded}
            >
              {expanded ? "Show less" : "Show more"}
            </button>
          )}
        </>
      )}

      <a
        href={destination}
        rel="noreferrer"
        className="mt-auto flex items-center gap-1 self-start text-xs text-muted-foreground hover:text-foreground hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {entry.reviewTruncated ? "Continue on Letterboxd" : "Open on Letterboxd"}
        <ExternalLink className="size-3" aria-hidden />
      </a>
    </article>
  )
}

export function LetterboxdReviewList({ reviews }: { reviews: LetterboxdReview[] }) {
  if (!reviews.length) return null
  return (
    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      {reviews.map((review) => (
        <ReviewCard key={review.profileId} entry={review} />
      ))}
    </div>
  )
}

export function LetterboxdReviews({ item }: { item: ItemDetail }) {
  const eligible = item.kind === "Movie" && Boolean(item.providerIds.tmdb)
  const lookup = useLetterboxdReviews(item.id, eligible)
  if (!eligible || lookup.error) return null

  if (lookup.isPending) {
    return (
      <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14" aria-label="Loading Letterboxd reviews">
        <h2 className="section-title">Letterboxd</h2>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          <Skeleton className="h-32 rounded-none" />
        </div>
      </section>
    )
  }

  const result = lookup.data
  if (!result || (!result.reviews.length && result.unavailableProfiles === 0)) return null
  return (
    <section className="flex flex-col gap-4 px-6 sm:px-10 lg:px-14" aria-labelledby="letterboxd-reviews-heading">
      <div>
        <h2 id="letterboxd-reviews-heading" className="section-title">Letterboxd</h2>
        {result.unavailableProfiles > 0 && (
          <p className="mt-2 text-xs text-muted-foreground">
            {result.unavailableProfiles === result.configuredProfiles
              ? "Connected profiles could not be refreshed."
              : `${result.unavailableProfiles} connected profile${result.unavailableProfiles === 1 ? " was" : "s were"} unavailable; cached entries are marked.`}
          </p>
        )}
      </div>
      <LetterboxdReviewList reviews={result.reviews} />
    </section>
  )
}
