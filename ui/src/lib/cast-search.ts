import type { PersonIdentity, SeerrResult } from "./api"

export interface CastSearchPerson {
  jellyfinId: string | null
  tmdbId: number | null
  name: string
}

function positiveInteger(value: string | null) {
  if (!value) return null
  const parsed = Number(value)
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null
}

/** Cast mode exists only when explicitly named; ordinary `search=` links stay ordinary. */
export function readCastSearch(params: URLSearchParams): CastSearchPerson | null {
  if (params.get("mode") !== "person") return null
  return {
    jellyfinId: params.get("personId")?.trim() || null,
    tmdbId: positiveInteger(params.get("tmdbPersonId")),
    name: params.get("personName")?.trim() || "Cast member",
  }
}

export function castSearchPath(person: {
  jellyfinId?: string | null
  tmdbId?: number | null
  name: string
}) {
  const params = new URLSearchParams({ mode: "person", personName: person.name.trim() })
  if (person.jellyfinId?.trim()) params.set("personId", person.jellyfinId.trim())
  if (person.tmdbId && Number.isSafeInteger(person.tmdbId) && person.tmdbId > 0) {
    params.set("tmdbPersonId", String(person.tmdbId))
  }
  return `/library?${params.toString()}`
}

/** Adds provider identities discovered after navigation without dropping route state. */
export function writeResolvedCastPerson(previous: URLSearchParams, person: PersonIdentity) {
  const next = new URLSearchParams(previous)
  next.set("mode", "person")
  next.set("personId", person.jellyfinId)
  next.set("personName", person.name)
  if (person.tmdbId) next.set("tmdbPersonId", String(person.tmdbId))
  else next.delete("tmdbPersonId")
  next.delete("search")
  return next
}

function mediaKey(result: Pick<SeerrResult, "mediaType" | "tmdbId">) {
  return `${result.mediaType}:${result.tmdbId}`
}

/**
 * Local titles belong to the complete Jellyfin section, never Discover. If a
 * malformed provider response repeats one, any owned copy suppresses the whole
 * key so another copy cannot misleadingly offer a request.
 */
export function castDiscoverResults(results: SeerrResult[] | undefined) {
  if (!results) return undefined
  const locallyAvailable = new Set(
    results.filter((result) => result.libraryItemId).map(mediaKey),
  )
  const seen = new Set<string>()
  return results.filter((result) => {
    const key = mediaKey(result)
    if (locallyAvailable.has(key) || seen.has(key)) return false
    seen.add(key)
    return true
  })
}
