import { Link } from "react-router-dom"
import { SeerrResults } from "@/components/seerr/SeerrResults"
import { castDiscoverResults } from "@/lib/cast-search"
import { useSeerrPersonCredits, useSeerrStatus, useStatus } from "@/lib/queries"

export function CastDiscover({
  personName,
  jellyfinId,
  tmdbId,
  resolving = false,
  resolutionError = null,
}: {
  personName: string
  jellyfinId: string | null
  tmdbId: number | null
  resolving?: boolean
  resolutionError?: Error | null
}) {
  const seerr = useSeerrStatus()
  const app = useStatus()
  const companionCapabilities = app.data?.companion?.info?.capabilities ?? []
  const companionProvidesSeerr = Boolean(
    app.data?.companion?.compatible && companionCapabilities.includes("seerr"),
  )
  const providerSupportsPeople =
    !companionProvidesSeerr || companionCapabilities.includes("seerr-person-discovery")
  const linked = seerr.data?.linked ?? false
  const catalogComplete = Boolean(app.data?.bootstrap?.complete ?? app.data?.bootstrapped)
  // With an exact Jellyfin person id the backend verifies every Seerr credit
  // against the live server relation. Without one, wait for the progressive
  // catalog to finish so an unseen local title is never offered as a request.
  const availabilitySafe = jellyfinId !== null || catalogComplete
  const credits = useSeerrPersonCredits(
    tmdbId,
    jellyfinId,
    linked
      && providerSupportsPeople
      && tmdbId !== null
      && !resolving
      && !resolutionError
      && availabilitySafe,
  )
  const results = castDiscoverResults(credits.data?.results)

  let content
  if (seerr.isPending || app.isPending) {
    content = <p className="py-4 text-sm text-muted-foreground">Checking Seerr…</p>
  } else if (seerr.error) {
    content = (
      <p className="py-4 text-sm text-destructive">
        Seerr is unavailable: {seerr.error.message}
      </p>
    )
  } else if (!linked) {
    content = (
      <p className="py-4 text-sm text-muted-foreground">
        Seerr is not connected. Your server results above are still complete.{" "}
        <Link to="/settings/integrations/seerr" className="text-primary hover:underline">
          Open Seerr settings
        </Link>
        .
      </p>
    )
  } else if (!providerSupportsPeople) {
    content = (
      <p className="py-4 text-sm text-muted-foreground">
        Update the MediaFlick Companion plugin to discover this person’s Seerr titles.
      </p>
    )
  } else if (resolving) {
    content = <p className="py-4 text-sm text-muted-foreground">Matching this person with Jellyfin and TMDB…</p>
  } else if (resolutionError) {
    content = (
      <p className="py-4 text-sm text-destructive">
        This person’s Seerr identity could not be verified: {resolutionError.message}
      </p>
    )
  } else if (tmdbId === null) {
    content = (
      <p className="py-4 text-sm text-muted-foreground">
        Jellyfin has no TMDB identity for this cast member, so Seerr discovery cannot be matched safely.
      </p>
    )
  } else if (!availabilitySafe) {
    content = (
      <p className="py-4 text-sm text-muted-foreground">
        Finishing the progressive library catalog before showing requestable titles, so local titles are never duplicated here.
      </p>
    )
  } else {
    content = (
      <SeerrResults
        results={results}
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
          More films and series featuring {personName}, with Seerr availability and request status.
        </p>
      </div>
      {content}
    </section>
  )
}
