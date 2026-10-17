type RatingOrigin = "plugin"

export interface RatingSourceDefinition {
  id: string
  label: string
  shortLabel: string
  scaleMax: number
  format: "percent" | "decimal" | "integer" | "stars" | (string & {})
  known: boolean
}

export interface RatingsIntegrationStatus {
  boundaryVersion: 1
  effectiveOrigin: RatingOrigin | "none"
  available: boolean
  selectionEnabled: boolean
  plugin: {
    available: boolean
    capability: "ratings-v1"
    boundaryVersion: 1
    detail: string
  }
  sources: RatingSourceDefinition[]
  selectedSources: string[]
}

export interface NormalizedRating {
  sourceId: string
  value: number
  score: number | null
  votes: number | null
  scaleMax: number
}

export interface ItemRatings {
  id: string
  ratings: NormalizedRating[]
  origin: RatingOrigin
  fetchedAt: number
  sourceUpdatedAt: string | null
  stale: boolean
  schemaVersion: number
}

export interface RatingsBatchResponse {
  available: boolean
  effectiveOrigin: RatingOrigin | "none"
  items: ItemRatings[]
  retryAt: number | null
  quota?: {
    limit: number | null
    remaining: number | null
    resetAt: number | null
  }
  diagnostic: string | null
}
