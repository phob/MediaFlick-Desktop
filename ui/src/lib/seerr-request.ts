import type {
  SeerrCapabilities,
  SeerrMediaType,
  SeerrResult,
  SeerrStatus,
} from "./api"

function isQuickRequestableStatus(mediaType: SeerrMediaType, status: SeerrStatus) {
  return status === "unknown" || (mediaType === "tv" && status === "partial")
}

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
