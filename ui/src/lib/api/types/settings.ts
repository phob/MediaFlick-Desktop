import type * as Wire from "../wire.gen.ts"
import { sameJson } from "../../json.ts"

/**
 * A settings patch as a shelf sends it: every key, each typed as the shelf's
 * view types it. Rust also takes `null` for "keep the stored value", which no
 * shelf sends.
 */
type SettingsWrite<Patch, View extends Record<keyof Patch, unknown>> = Patch & Pick<View, keyof Patch>

export type PlayerSettings = Wire.PlayerView

export type PlayerSettingsWrite = SettingsWrite<Wire.PlayerSettingsPatch, PlayerSettings>

export function playerSettingsWrite(settings: PlayerSettings): PlayerSettingsWrite {
  return {
    playerBackend: settings.playerBackend,
    mpvPath: settings.mpvPath,
    defaultFullscreen: settings.defaultFullscreen,
    markWatchedNext: settings.markWatchedNext,
    comfort: settings.comfort,
  }
}

export type StartupDestination = Wire.StartupView

export type ViewingSettings = Wire.ViewingView

export type ViewingSettingsWrite = Wire.ViewingSettingsWrite

export function viewingSettingsWrite(settings: ViewingSettings, source: ViewingSettings | undefined): ViewingSettingsWrite {
  const { startupDestination, ...others } = settings
  return source && sameJson(startupDestination, source.startupDestination) ? others : settings
}

export type PlayerComfort = Wire.PlayerComfort

export type ClientSettings = Wire.SettingsView

export type AppearanceSettings = Wire.AppearanceView

export type PlaybackSettingsWrite = SettingsWrite<Wire.PlaybackSettingsPatch, Wire.PlaybackView>

export type ApplicationSettingsWrite = SettingsWrite<Wire.ApplicationSettingsPatch, Wire.ApplicationView>

export type AppearanceSettingsWrite = SettingsWrite<Wire.AppearanceSettingsPatch, AppearanceSettings>

export type StreamingQualityId = Wire.StreamingQuality

export const STREAMING_QUALITIES: { id: StreamingQualityId; label: string }[] = [
  { id: "original", label: "Original file" },
  { id: "auto", label: "Auto" },
  { id: "120_mbps", label: "120 Mbps" },
  { id: "80_mbps", label: "80 Mbps" },
  { id: "60_mbps", label: "60 Mbps" },
  { id: "40_mbps", label: "40 Mbps" },
  { id: "20_mbps", label: "20 Mbps" },
  { id: "10_mbps", label: "10 Mbps" },
  { id: "5_mbps", label: "5 Mbps" },
  { id: "3_mbps", label: "3 Mbps" },
  { id: "1_5_mbps", label: "1.5 Mbps" },
]

export function qualityLabel(id: string | null | undefined) {
  return STREAMING_QUALITIES.find((quality) => quality.id === id)?.label ?? null
}
