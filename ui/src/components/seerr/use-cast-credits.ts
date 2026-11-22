import { useSeerrPersonCredits, useSeerrStatus } from "@/lib/seerr-queries"
import { useStatus } from "@/lib/session-queries"
import { useCompanionCapabilities } from "@/lib/companion"

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
  const app = useStatus()
  const companion = useCompanionCapabilities()
  const companionProvidesSeerr = companion.has("seerr")
  const seerr = useSeerrStatus(companionProvidesSeerr)
  const providerSupportsPeople =
    !companionProvidesSeerr || companion.has("seerr-person-discovery")
  const linked = seerr.data?.linked ?? false
  const catalogComplete = app.data?.catalogComplete ?? false
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
  if (app.isPending || companion.isPending) phase = "checking-seerr"
  else if (!companionProvidesSeerr) phase = "not-linked"
  else if (seerr.isPending) phase = "checking-seerr"
  else if (seerr.error) phase = "seerr-error"
  else if (!linked) phase = "not-linked"
  else if (!providerSupportsPeople) phase = "needs-plugin"
  else if (resolving) phase = "resolving"
  else if (resolutionError) phase = "resolution-error"
  else if (tmdbId === null) phase = "no-tmdb-identity"
  else if (!availabilitySafe) phase = "waiting-for-catalog"

  return { credits, phase, seerrError: seerr.error }
}
