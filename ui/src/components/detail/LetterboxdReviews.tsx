import { ExternalLink, Star } from "lucide-react"
import { Popover as PopoverPrimitive } from "radix-ui"
import {
  type FocusEvent,
  type KeyboardEvent,
  type PointerEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react"
import { LetterboxdMark } from "@/components/RatingSourceIcon"
import { Skeleton } from "@/components/ui/skeleton"
import type {
  ItemDetail,
  LetterboxdReview,
  LetterboxdReviewsResponse,
  SeerrMediaType,
} from "@/lib/api"
import { useLetterboxdMovieReviews, useLetterboxdReviews } from "@/lib/queries"

const REVIEW_PREVIEW_CHARS = 420
const REVIEW_OPEN_DELAY_MS = 350
const REVIEW_CLOSE_DELAY_MS = 150
const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",")

function ratingLabel(value: number) {
  return Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1)
}

function reviewPreview(review: string) {
  const characters = Array.from(review)
  if (characters.length <= REVIEW_PREVIEW_CHARS) return review

  let excerpt = characters.slice(0, REVIEW_PREVIEW_CHARS).join("").trimEnd()
  const lastBreak = Math.max(excerpt.lastIndexOf(" "), excerpt.lastIndexOf("\n"))
  if (lastBreak >= REVIEW_PREVIEW_CHARS * 0.75) excerpt = excerpt.slice(0, lastBreak).trimEnd()
  return `${excerpt}…`
}

function nextFocusableAfter(element: HTMLElement) {
  const focusable = Array.from(document.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (candidate) =>
      candidate.getAttribute("aria-hidden") !== "true" &&
      !candidate.closest("[data-letterboxd-review-preview]"),
  )
  const index = focusable.indexOf(element)
  return index >= 0 ? (focusable[index + 1] ?? null) : null
}

function RatingStars({ entry }: { entry: LetterboxdReview }) {
  if (entry.rating == null) return <div className="h-4" aria-hidden />

  const label = `${ratingLabel(entry.rating)} out of 5 stars from ${entry.displayName}`
  return (
    <div className="flex h-4 items-center justify-center gap-0.5" role="img" aria-label={label}>
      {Array.from({ length: 5 }, (_, index) => {
        const fill = Math.max(0, Math.min(1, entry.rating! - index))
        return (
          <span key={index} className="relative size-3.5 text-muted-foreground/35" aria-hidden>
            <Star className="absolute inset-0 size-3.5" strokeWidth={1.75} />
            {fill > 0 && (
              <span
                className="absolute inset-y-0 left-0 overflow-hidden text-[#00e054]"
                style={{ width: `${fill * 100}%` }}
              >
                <Star className="size-3.5 fill-current" strokeWidth={1.75} />
              </span>
            )}
          </span>
        )
      })}
    </div>
  )
}

function ProfileIdentity({ entry }: { entry: LetterboxdReview }) {
  return (
    <>
      <span className="grid size-24 place-items-center rounded-full bg-[#14181c] ring-1 ring-white/10">
        <LetterboxdMark className="size-16" />
      </span>
      <RatingStars entry={entry} />
      <span className="line-clamp-2 min-h-7 text-xs leading-tight" title={entry.displayName}>
        {entry.displayName}
      </span>
    </>
  )
}

function tileLabel(entry: LetterboxdReview) {
  const identity = `${entry.displayName} (@${entry.username})`
  const rating = entry.rating == null ? "" : `, rated ${ratingLabel(entry.rating)} out of 5 stars`
  const review = entry.review ? ", written review available" : ""
  return `${identity}${rating}${review} on Letterboxd. Open on Letterboxd.`
}

function ProfileTile({ entry }: { entry: LetterboxdReview }) {
  const [open, setOpen] = useState(false)
  const descriptionId = useId()
  const triggerRef = useRef<HTMLAnchorElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const reviewLinkRef = useRef<HTMLAnchorElement>(null)
  const openTimerRef = useRef<number | null>(null)
  const closeTimerRef = useRef<number | null>(null)
  const destination = entry.entryUrl ?? entry.profileUrl
  const preview = entry.review ? reviewPreview(entry.review) : null

  const clearOpenTimer = useCallback(() => {
    if (openTimerRef.current == null) return
    window.clearTimeout(openTimerRef.current)
    openTimerRef.current = null
  }, [])
  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current == null) return
    window.clearTimeout(closeTimerRef.current)
    closeTimerRef.current = null
  }, [])
  const close = useCallback(() => {
    clearOpenTimer()
    clearCloseTimer()
    setOpen(false)
  }, [clearCloseTimer, clearOpenTimer])
  const closeAndRestoreFocus = useCallback(() => {
    close()
    triggerRef.current?.focus()
  }, [close])

  useEffect(
    () => () => {
      clearOpenTimer()
      clearCloseTimer()
    },
    [clearCloseTimer, clearOpenTimer],
  )

  useEffect(() => {
    if (!open) return
    const closeForViewportChange = () => {
      const focusWasInPreview = contentRef.current?.contains(document.activeElement)
      close()
      if (focusWasInPreview) triggerRef.current?.focus()
    }
    window.addEventListener("scroll", closeForViewportChange, true)
    window.addEventListener("resize", closeForViewportChange)
    return () => {
      window.removeEventListener("scroll", closeForViewportChange, true)
      window.removeEventListener("resize", closeForViewportChange)
    }
  }, [close, open])

  const scheduleOpen = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === "touch") return
    clearCloseTimer()
    clearOpenTimer()
    openTimerRef.current = window.setTimeout(() => setOpen(true), REVIEW_OPEN_DELAY_MS)
  }
  const scheduleClose = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === "touch") return
    clearOpenTimer()
    clearCloseTimer()
    closeTimerRef.current = window.setTimeout(() => {
      const focused = document.activeElement
      const focusRemainsInside =
        triggerRef.current === focused ||
        (focused instanceof Node && Boolean(contentRef.current?.contains(focused)))
      if (!focusRemainsInside) setOpen(false)
    }, REVIEW_CLOSE_DELAY_MS)
  }
  const keepOpen = () => {
    clearOpenTimer()
    clearCloseTimer()
    setOpen(true)
  }
  const leaveTrigger = (event: FocusEvent<HTMLAnchorElement>) => {
    const next = event.relatedTarget
    if (next instanceof Node && contentRef.current?.contains(next)) return
    close()
  }
  const leavePreview = (event: FocusEvent<HTMLDivElement>) => {
    const next = event.relatedTarget
    if (
      next instanceof Node &&
      (contentRef.current?.contains(next) || triggerRef.current?.contains(next))
    ) {
      return
    }
    close()
  }
  const enterPreviewFromTrigger = (event: KeyboardEvent<HTMLAnchorElement>) => {
    if (event.key !== "Tab" || event.shiftKey || !open || !reviewLinkRef.current) return
    event.preventDefault()
    reviewLinkRef.current.focus()
  }
  const leavePreviewLink = (event: KeyboardEvent<HTMLAnchorElement>) => {
    if (event.key === "Escape") {
      event.preventDefault()
      closeAndRestoreFocus()
      return
    }
    if (event.key !== "Tab") return
    if (event.shiftKey) {
      event.preventDefault()
      closeAndRestoreFocus()
      return
    }
    const next = triggerRef.current ? nextFocusableAfter(triggerRef.current) : null
    close()
    if (next) {
      event.preventDefault()
      next.focus()
    }
  }

  const tile = (
    <a
      ref={triggerRef}
      href={destination}
      rel="noreferrer"
      data-letterboxd-profile={entry.profileId}
      aria-label={tileLabel(entry)}
      aria-describedby={preview ? descriptionId : undefined}
      title={entry.displayName}
      onPointerEnter={preview ? scheduleOpen : undefined}
      onPointerLeave={preview ? scheduleClose : undefined}
      onFocus={preview ? keepOpen : undefined}
      onBlur={preview ? leaveTrigger : undefined}
      onKeyDown={preview ? enterPreviewFromTrigger : undefined}
      className="flex w-28 shrink-0 flex-col items-center gap-2 rounded-media text-center outline-none transition-colors hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
    >
      <ProfileIdentity entry={entry} />
    </a>
  )

  if (!preview) return <div role="listitem">{tile}</div>

  return (
    <div role="listitem">
      <span id={descriptionId} className="sr-only">
        {preview}
      </span>
      <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
        <PopoverPrimitive.Anchor asChild>{tile}</PopoverPrimitive.Anchor>
        <PopoverPrimitive.Portal>
          <PopoverPrimitive.Content
            ref={contentRef}
            data-letterboxd-review-preview
            side="top"
            align="center"
            sideOffset={12}
            collisionPadding={16}
            onOpenAutoFocus={(event) => event.preventDefault()}
            onCloseAutoFocus={(event) => event.preventDefault()}
            onPointerEnter={keepOpen}
            onPointerLeave={scheduleClose}
            onFocusCapture={keepOpen}
            onBlurCapture={leavePreview}
            onEscapeKeyDown={(event) => {
              event.preventDefault()
              closeAndRestoreFocus()
            }}
            aria-label={`${entry.displayName}'s Letterboxd review`}
            className="z-50 w-[min(24rem,calc(100vw-2rem))] origin-(--radix-popover-content-transform-origin) animate-in rounded-media border border-border/80 bg-popover/95 p-4 text-popover-foreground shadow-2xl shadow-black/40 backdrop-blur-xl fade-in-0 zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95"
          >
            <div className="flex flex-col gap-3">
              <div className="min-w-0">
                <p className="truncate text-sm font-semibold">{entry.displayName}</p>
                {entry.watchedDate && (
                  <p className="data-value mt-1 text-[0.65rem] text-muted-foreground">
                    Reviewed {entry.watchedDate}
                  </p>
                )}
              </div>
              <blockquote className="line-clamp-6 whitespace-pre-line text-sm leading-relaxed text-foreground/90">
                {preview}
              </blockquote>
              <a
                ref={reviewLinkRef}
                href={destination}
                rel="noreferrer"
                onKeyDown={leavePreviewLink}
                className="flex items-center gap-1 self-start text-xs font-medium text-primary hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                aria-label={`Read ${entry.displayName}'s review on Letterboxd`}
              >
                Read on Letterboxd
                <ExternalLink className="size-3" aria-hidden />
              </a>
            </div>
          </PopoverPrimitive.Content>
        </PopoverPrimitive.Portal>
      </PopoverPrimitive.Root>
    </div>
  )
}

export function LetterboxdReviewList({ reviews }: { reviews: LetterboxdReview[] }) {
  if (!reviews.length) return null
  return (
    <div
      className="media-strip flex gap-6 overflow-x-auto px-6 pb-3 sm:px-10 lg:px-14"
      role="list"
      aria-label="Connected Letterboxd profiles with activity for this movie"
    >
      {reviews.map((review) => (
        <ProfileTile key={review.profileId} entry={review} />
      ))}
    </div>
  )
}

function LetterboxdLoading() {
  return (
    <section className="flex min-w-0 flex-col gap-4" aria-label="Loading Letterboxd activity">
      <h2 className="section-title px-6 sm:px-10 lg:px-14">Letterboxd</h2>
      <div className="media-strip flex gap-6 overflow-x-auto px-6 pb-3 sm:px-10 lg:px-14">
        {Array.from({ length: 3 }, (_, index) => (
          <div key={index} className="flex w-28 shrink-0 flex-col items-center gap-2" aria-hidden>
            <Skeleton className="size-24 rounded-full" />
            <Skeleton className="h-4 w-20 rounded-none" />
            <Skeleton className="h-3 w-16 rounded-none" />
          </div>
        ))}
      </div>
    </section>
  )
}

function UnavailableProfiles({ unavailable, configured }: { unavailable: number; configured: number }) {
  if (unavailable <= 0) return null
  const allUnavailable = configured > 0 && unavailable === configured
  return (
    <p className="px-6 text-xs text-muted-foreground sm:px-10 lg:px-14" role="status">
      {allUnavailable
        ? "Connected profiles could not be refreshed."
        : `${unavailable} connected profile${unavailable === 1 ? " was" : "s were"} unavailable; cached activity remains visible.`}
    </p>
  )
}

function LetterboxdReviewSection({
  lookup,
}: {
  lookup: {
    isPending: boolean
    error: Error | null
    data: LetterboxdReviewsResponse | undefined
  }
}) {
  if (lookup.isPending) return <LetterboxdLoading />

  if (lookup.error) {
    return (
      <section className="flex min-w-0 flex-col gap-4" aria-labelledby="letterboxd-reviews-heading">
        <h2 id="letterboxd-reviews-heading" className="section-title px-6 sm:px-10 lg:px-14">
          Letterboxd
        </h2>
        <p className="px-6 text-xs text-muted-foreground sm:px-10 lg:px-14" role="status">
          Connected profiles could not be refreshed.
        </p>
      </section>
    )
  }

  const result = lookup.data
  if (!result || (!result.reviews.length && result.unavailableProfiles === 0)) return null
  return (
    <section className="flex min-w-0 flex-col gap-4" aria-labelledby="letterboxd-reviews-heading">
      <h2 id="letterboxd-reviews-heading" className="section-title px-6 sm:px-10 lg:px-14">
        Letterboxd
      </h2>
      <UnavailableProfiles
        unavailable={result.unavailableProfiles}
        configured={result.configuredProfiles}
      />
      <LetterboxdReviewList reviews={result.reviews} />
    </section>
  )
}

export function LetterboxdReviews({ item }: { item: ItemDetail }) {
  const eligible = item.kind === "Movie" && Boolean(item.providerIds.tmdb)
  const lookup = useLetterboxdReviews(item.id, eligible)
  if (!eligible) return null
  return <LetterboxdReviewSection lookup={lookup} />
}

export function LetterboxdMovieReviews({ tmdbId }: { tmdbId: number }) {
  const eligible = Number.isSafeInteger(tmdbId) && tmdbId > 0
  const lookup = useLetterboxdMovieReviews(tmdbId, eligible)
  if (!eligible) return null
  return <LetterboxdReviewSection lookup={lookup} />
}

export function DiscoverLetterboxdReviews({
  mediaType,
  tmdbId,
}: {
  mediaType: SeerrMediaType
  tmdbId: number
}) {
  if (mediaType !== "movie") return null
  return <LetterboxdMovieReviews tmdbId={tmdbId} />
}
