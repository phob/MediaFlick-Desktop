import type * as Wire from "../wire.gen.ts"

export type HomeElement = Wire.HomeElementView

export type HomeConfiguration = Wire.HomeConfiguration

export type HomeSettingsResponse = Wire.HomeSettingsResponse

export type HomeSettingsWrite = Wire.HomeSettings

export function homeSettingsWrite(settings: HomeConfiguration): HomeSettingsWrite {
  return {
    billboard: settings.billboard,
    watching: settings.watching,
    elements: settings.elements.map(homeElementWrite),
  }
}

function homeElementWrite(element: HomeElement): Wire.HomeElement {
  const { enabled } = element
  switch (element.kind) {
    case "builtIn": return { kind: element.kind, id: element.id, enabled }
    case "genre": return { kind: element.kind, id: element.id, enabled }
    case "collection": return { kind: element.kind, id: element.id, enabled }
    case "library": return { kind: element.kind, id: element.id, enabled }
  }
}

export type HomeRow = Wire.HomeRow

export type HomeResponse = Wire.HomeResponse

export type HomeResumeResponse = Wire.HomeResumeResponse
