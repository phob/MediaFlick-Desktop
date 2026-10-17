import type * as Wire from "../wire.gen.ts"
import type { CollectionMode } from "./collections.ts"

export type HomeElement = Wire.HomeElementView

export type HomeConfiguration = Wire.HomeConfiguration

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

export type HomeRow = Wire.HomeRow

export type HomeResponse = Wire.HomeResponse

export type HomeResumeResponse = Wire.HomeResumeResponse
