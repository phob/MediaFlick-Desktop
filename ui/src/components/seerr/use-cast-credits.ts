import { useSeerrPersonCredits, useSeerrStatus, useStatus } from "@/lib/queries"

/**
 * Everything both cast surfaces (the Discover row and the proven server
 * extras) need before person credits may be requested, and the one phase the
 * UI should describe while they wait.
 */
export type CastCreditsPhase =
  | "checking-seerr"
  | "seerr-error"
  | "not-linked"
  | "needs-plugin"
  | "resolving"
  | "resolution-error"
  | "no-tmdb-identity"
  | "waiting-for-catalog"
  | "ready"

export interface CastCreditsInput {
  jellyfinId: string | null
  tmdbId: number | null
  resolving?: boolean
  resolutionError?: Error | null
}

export function useCastCredits({
  jellyfinId,
  tmdbId,
  resolving = false,
  resolutionError = null,
}: CastCreditsInput) {
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

  let phase: CastCreditsPhase = "ready"
  if (seerr.isPending || app.isPending) phase = "checking-seerr"
  else if (seerr.error) phase = "seerr-error"
  else if (!linked) phase = "not-linked"
  else if (!providerSupportsPeople) phase = "needs-plugin"
  else if (resolving) phase = "resolving"
  else if (resolutionError) phase = "resolution-error"
  else if (tmdbId === null) phase = "no-tmdb-identity"
  else if (!availabilitySafe) phase = "waiting-for-catalog"

  return { credits, phase }
}
