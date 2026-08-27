import { Link } from "react-router-dom"
import { SeerrResults } from "@/components/seerr/SeerrResults"
import { castDiscoverResults } from "@/lib/cast-search"
import type { CastCreditsInput, CastCreditsPhase } from "@/components/seerr/use-cast-credits"
import { useCastCredits } from "@/components/seerr/use-cast-credits"

export interface CastDiscoverProps extends CastCreditsInput {
  personName: string
}

const PHASE_COPY = {
  "checking-seerr": "Checking Seerr…",
  "not-linked":
    "Seerr is not connected. Your server results above are still complete. ",
  "needs-plugin": "Seerr person discovery is unavailable on this server.",
  resolving: "Matching this person with Jellyfin and TMDB…",
  "waiting-for-catalog":
    "Finishing the progressive library catalog before showing requestable titles, so local titles are never duplicated here.",
} satisfies Partial<Record<CastCreditsPhase, string>>

export function CastDiscover({ personName, ...creditsInput }: CastDiscoverProps) {
  const { credits, phase } = useCastCredits(creditsInput)

  let content
  if (phase === "seerr-error") {
    content = (
      <p className="py-4 text-sm text-destructive">
        Seerr is unavailable: {credits.error?.message ?? "unknown error"}
      </p>
    )
  } else if (phase === "resolution-error") {
    content = (
      <p className="py-4 text-sm text-destructive">
        This person’s Seerr identity could not be verified: {creditsInput.resolutionError?.message}
      </p>
    )
  } else if (phase === "no-tmdb-identity") {
    content = (
      <p className="py-4 text-sm text-muted-foreground">
        Jellyfin has no TMDB identity for this cast member, so Seerr discovery cannot be matched safely.
      </p>
    )
  } else if (phase !== "ready") {
    const copy = PHASE_COPY[phase] ?? "Checking Seerr…"
    content =
      phase === "not-linked" ? (
        <p className="py-4 text-sm text-muted-foreground">
          {copy}
          <Link to="/" className="text-primary hover:underline">
            Back to Home
          </Link>
          .
        </p>
      ) : (
        <p className="py-4 text-sm text-muted-foreground">{copy}</p>
      )
  } else {
    content = (
      <SeerrResults
        results={castDiscoverResults(credits.data?.results)}
        isPending={credits.isPending}
        error={credits.error}
        placeholders={4}
        empty={`No additional Seerr titles featuring ${personName} were found.`}
      />
    )
  }

  return (
    <section className="flex flex-col gap-3 pt-10" aria-labelledby="cast-discover-heading">
      <div>
        <h2 id="cast-discover-heading" className="section-title">Discover</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          More movies and series featuring {personName}, with Seerr availability and request status.
        </p>
      </div>
      {content}
    </section>
  )
}
