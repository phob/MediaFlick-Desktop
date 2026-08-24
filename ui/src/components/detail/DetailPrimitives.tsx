import { ArrowLeft, UserRound } from "lucide-react"
import { useState, type ReactNode } from "react"
import { Link, type To } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"

export function DetailPageSkeleton() {
  return (
    <div className="flex min-h-[26rem] gap-8 px-6 pt-14 pb-12 sm:px-10 lg:px-14">
      <Skeleton className="hidden h-[330px] w-[220px] shrink-0 rounded-xl sm:block" />
      <div className="flex flex-1 flex-col gap-4">
        <Skeleton className="h-4 w-32" />
        <Skeleton className="h-12 w-2/3 max-w-2xl" />
        <Skeleton className="h-4 w-72" />
        <Skeleton className="h-24 max-w-3xl" />
        <Skeleton className="h-11 w-72" />
      </div>
    </div>
  )
}

/**
 * Full-page backdrop behind a detail route. The artwork spans the entire page
 * instead of stopping at the header, so every section scrolls over one
 * continuous image, cropped to fill. A light even veil keeps content readable
 * over the art, and a soft fade protects the header text at the top. The art
 * stays visible to the page's end — no scrim band cuts it off.
 */
export function DetailBackdrop({ src }: { src: string }) {
  const [failed, setFailed] = useState(false)
  if (failed) return null
  return (
    <div className="pointer-events-none absolute inset-0 -z-10 overflow-hidden" aria-hidden>
      <img
        src={src}
        alt=""
        decoding="async"
        onError={() => setFailed(true)}
        className="media-backdrop-image h-full w-full object-cover"
      />
      <div className="absolute inset-0 bg-background/30" />
      <div className="absolute inset-x-0 top-0 h-80 bg-linear-to-b from-background/90 via-background/40 to-transparent" />
    </div>
  )
}

export function DetailBackLink({ to, label }: { to: To; label: string }) {
  return (
    <Button variant="ghost" size="sm" className="w-fit px-0 text-muted-foreground" asChild>
      <Link to={to}>
        <ArrowLeft aria-hidden />
        {label}
      </Link>
    </Button>
  )
}

export function DetailFact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[8rem_1fr] gap-4 py-2.5 text-sm">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  )
}

export function DetailFactPanel({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <dl className={cn("divide-y divide-border/60 rounded-xl border border-border/60 bg-card px-4 py-1 shadow-lg shadow-black/40", className)}>
      {children}
    </dl>
  )
}

export interface DetailCastEntry {
  key: string
  name: string
  role?: string | null
  imageUrl?: string | null
  to: string
}

function Headshot({ entry }: { entry: DetailCastEntry }) {
  const [failed, setFailed] = useState(false)
  if (!entry.imageUrl || failed) {
    return (
      <div className="grid size-24 place-items-center rounded-full bg-card text-muted-foreground ring-1 ring-white/10">
        <UserRound className="size-8" aria-hidden />
      </div>
    )
  }
  return (
    <img
      src={entry.imageUrl}
      alt=""
      decoding="async"
      loading="lazy"
      onError={() => setFailed(true)}
      className="size-24 rounded-full object-cover ring-1 ring-white/10"
    />
  )
}

export function DetailCastRail({ entries }: { entries: DetailCastEntry[] }) {
  if (!entries.length) return null
  return (
    <section className="flex min-w-0 flex-col gap-4">
      <h2 className="section-title px-6 sm:px-10 lg:px-14">Cast</h2>
      <div className="media-strip flex gap-6 overflow-x-auto px-6 pb-3 sm:px-10 lg:px-14">
        {entries.map((entry) => (
          <Link
            key={entry.key}
            to={entry.to}
            aria-label={`Find titles featuring ${entry.name}`}
            className="flex w-28 shrink-0 flex-col items-center gap-2 rounded-media text-center outline-none transition-colors hover:text-primary focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Headshot entry={entry} />
            {/* The labels float over the page backdrop, so they carry their own
                contrast shadow for light artwork. */}
            <span className="flex flex-col gap-0.5 drop-shadow-[0_1px_2px_rgba(0,0,0,0.9)]">
              <span className="text-xs leading-tight">{entry.name}</span>
              {entry.role && (
                <span className="text-xs leading-tight text-muted-foreground">{entry.role}</span>
              )}
            </span>
          </Link>
        ))}
      </div>
    </section>
  )
}
