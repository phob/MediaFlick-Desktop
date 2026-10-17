import type { JsonObject } from "../../json.ts"
import type { ItemSummary } from "./catalog.ts"

export type CollectionMode = "mediaFlick" | "jellyfin"

export type CollectionMediaType = "movie" | "series" | "mixed"

export type RefreshCadence = "manual" | "daily" | "weekly" | "monthly"

export type CollectionCategory =
  | "trending"
  | "popular"
  | "streamingServices"
  | "topRated"
  | "inTheaters"
  | "upcoming"
  | "onAir"
  | "editorial"
  | "custom"

export type CollectionTemplatePictogram =
  | "award"
  | "binary"
  | "blocks"
  | "bone"
  | "bookOpen"
  | "briefcase"
  | "bug"
  | "calendarClock"
  | "calendarDays"
  | "circle"
  | "compass"
  | "crosshair"
  | "drama"
  | "film"
  | "flame"
  | "ghost"
  | "heart"
  | "landmark"
  | "languages"
  | "laugh"
  | "listVideo"
  | "monitorPlay"
  | "mountain"
  | "music"
  | "orbit"
  | "palette"
  | "pawPrint"
  | "popcorn"
  | "rocket"
  | "search"
  | "slidersHorizontal"
  | "sparkles"
  | "star"
  | "swords"
  | "telescope"
  | "trendingUp"
  | "trophy"
  | "tv"
  | "usersRound"
  | "wandSparkles"
  | "zap"

export interface ProviderReadiness {
  tmdb: boolean
  mdblist: boolean
}

export interface CollectionSettings {
  effectiveMode: CollectionMode
  mediaFlickAvailable: boolean
  modeSelection: CollectionMode | null
  franchises: { includeUnreleased: boolean }
  readiness: ProviderReadiness
  recovery: { damagedPath: string; restoredBackup: boolean } | null
}

export type CollectionSource =
  | {
      kind: "tmdbDiscover"
      parameters: JsonObject
    }
  | {
      kind: "tmdbCollection"
      collectionId: number
      includeUnreleased: boolean
    }
  | {
      kind: "mdbListPublicList"
      listId: string
    }

export type CollectionResultLimit =
  | { kind: "all" }
  | { kind: "maximum"; count: number }

export interface CollectionTemplateReference {
  id: string
}

export interface CollectionProfileDraft {
  template: CollectionTemplateReference
  title: string
  description: string
  customPosterId: string | null
  source: CollectionSource
  mediaType: CollectionMediaType
  limit: CollectionResultLimit
  cadence: RefreshCadence
  availableOnHome: boolean
}

export interface CollectionProfile extends CollectionProfileDraft {
  id: string
  revision: string
}

export interface CollectionTemplate extends Omit<CollectionProfileDraft, "template" | "customPosterId" | "availableOnHome"> {
  id: string
  category: CollectionCategory
  pictogram: CollectionTemplatePictogram
}

export interface CollectionTemplates {
  categories: CollectionCategory[]
  templates: { template: CollectionTemplate; available: boolean }[]
  readiness: ProviderReadiness
}

export interface NormalizedCollectionTitle {
  mediaType: CollectionMediaType
  tmdbId: number
  title: string
  originalTitle?: string | null
  year?: number | null
  overview: string
  releaseDate?: string | null
  sourceOrder: number
  posterPath?: string | null
  backdropPath?: string | null
  adult: boolean
}

export interface ClassifiedCollectionTitle extends NormalizedCollectionTitle {
  localItems: { id: string; name: string; kind: string; played: boolean }[]
}

export interface CollectionPreview {
  items: NormalizedCollectionTitle[]
  total: number
  movies: number
  series: number
  sourceIdentity?: string | null
}

export interface PublicCollectionList {
  id: string
  name: string
  owner: string | null
}

export interface CollectionProfilesIndex {
  profiles: CollectionProfile[]
  errors?: Record<string, string>
}

export interface CollectionProfileDetail {
  profile: CollectionProfile
  status: "updating" | "resultsUnavailable" | "ready"
  owned: ClassifiedCollectionTitle[]
  missing: NormalizedCollectionTitle[]
  items: NormalizedCollectionTitle[]
  libraryItems: ItemSummary[]
  ownershipAvailable?: boolean
  refresh?: {
    lastAttempt: number | null
    lastSuccess: number | null
    latestFailure: string | null
    nextDue: number | null
    initialized: boolean
  }
  overdue?: boolean
}

export interface FranchiseCollection {
  collectionId: number
  name: string
  posterPath: string | null
  backdropPath: string | null
  owned: ClassifiedCollectionTitle[]
  missing: NormalizedCollectionTitle[]
  items?: NormalizedCollectionTitle[]
  libraryItems: ItemSummary[]
  ownershipAvailable?: boolean
}

export interface FranchiseCollectionSummary {
  collectionId: number
  name: string
  posterPath: string | null
  backdropPath: string | null
  ownedCount: number
  missingCount: number
  ownershipAvailable: boolean
}

export interface FranchiseCollectionsIndex {
  status: "updating" | "resultsUnavailable" | "ready"
  franchises: FranchiseCollectionSummary[]
}

export interface JellyfinCollectionSummary {
  id: string
  name: string
  primaryImageTag: string | null
  backdropImageTag: string | null
  itemCount: number | null
}

export interface JellyfinCollectionDetail {
  id: string
  name: string
  primaryImageTag: string | null
  backdropImageTag: string | null
  items: ItemSummary[]
  totalRecordCount: number
}

export interface MovieCollection {
  tmdbId: number
  collection: { id: number; name: string } | null
}
