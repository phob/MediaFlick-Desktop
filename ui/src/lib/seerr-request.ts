import type {
  SeerrCapabilities,
  SeerrMediaType,
  SeerrResult,
  SeerrStatus,
} from "./api"

/**
 * A discovery result does not carry per-season state. `partial` is therefore
 * actionable only for a series: its detail call can identify the seasons that
 * are still unknown. For a movie, every state except `unknown` describes a
 * request that Seerr already owns and must not be offered again.
 */
function isQuickRequestableStatus(mediaType: SeerrMediaType, status: SeerrStatus) {
  return status === "unknown" || (mediaType === "tv" && status === "partial")
}

/**
 * Whether the overview card has at least one request path worth opening.
 *
 * Local-library membership suppresses an ordinary `unknown` path: the local
 * join is stronger evidence that the movie/show is already present than a
 * missing Seerr media row. It is not a blanket veto, though—a `partial` series
 * can still have seasons to request, and an owned title may still have a 4K
 * request path. Conversely, a fully requested/available result has no action,
 * rather than a disabled control that only repeats the status badge.
 */
export function canQuickRequest(
  result: SeerrResult,
  capabilities: SeerrCapabilities | null | undefined,
) {
  const regularStatus = isQuickRequestableStatus(result.mediaType, result.status)
  const regular =
    capabilities?.[result.mediaType].request &&
    regularStatus &&
    (!result.libraryItemId || result.status === "partial")
  const fourK =
    capabilities?.[result.mediaType === "movie" ? "movie4k" : "tv4k"].request &&
    isQuickRequestableStatus(result.mediaType, result.status4k)

  return Boolean(regular || fourK)
}
