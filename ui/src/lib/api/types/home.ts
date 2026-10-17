import type { ItemSummary } from "./catalog.ts"
import type { CollectionMode } from "./collections.ts"

export type HomeBuiltInId =
  | "watching"
  | "becauseYouWatched"
  | "recentlyAdded"
  | "recentlyAddedShows"
  | "upcoming"
  | "latestMovies"
  | "latestShows"
  | "myList"

export type HomeElement = {
  enabled: boolean
  label: string
  available: boolean
  category: "Built-in" | "Genre" | "My Collection" | "Library"
} & (
  | { kind: "builtIn"; id: HomeBuiltInId }
  | { kind: "genre"; id: string }
  | { kind: "collection"; id: string }
  | { kind: "library"; id: string }
)

export interface HomeConfiguration {
  billboard: boolean
  watching: {
    continueWatching: boolean
    nextUp: boolean
    combine: boolean
  }
  elements: HomeElement[]
}

export interface HomeSettingsResponse {
  settings: HomeConfiguration
  defaults: HomeConfiguration
  collectionMode: CollectionMode
}

export type HomeSettingsWrite = Omit<HomeConfiguration, "elements"> & {
  elements: Array<Pick<HomeElement, "kind" | "id" | "enabled">>
}

export function homeSettingsWrite(settings: HomeConfiguration): HomeSettingsWrite {
  return {
    billboard: settings.billboard,
    watching: settings.watching,
    elements: settings.elements.map(({ kind, id, enabled }) => ({ kind, id, enabled })),
  }
}

export interface HomeRow {
  kind: "builtIn" | "genre" | "collection"
  id: string
  title: string
  items: ItemSummary[]
}

export interface HomeResponse {
  configuration: HomeConfiguration
  continueWatching: ItemSummary[]
  rows: HomeRow[]
}

export interface HomeResumeResponse {
  continueWatching: ItemSummary[]
  nextUp: ItemSummary[]
}
