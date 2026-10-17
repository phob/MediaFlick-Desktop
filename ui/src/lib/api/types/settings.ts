import { sameJson } from "../../json.ts"

export interface PlayerSettings {
  comfort: PlayerComfort
  playerBackend: "libmpv" | "mpv"
  mpvPath: string | null
  defaultFullscreen: "fullscreen" | "windowed"
  markWatchedNext: string | null
  playerConfigured: boolean
}

export type PlayerSettingsWrite = Omit<PlayerSettings, "playerConfigured">

export function playerSettingsWrite(settings: PlayerSettings): PlayerSettingsWrite {
  return {
    playerBackend: settings.playerBackend,
    mpvPath: settings.mpvPath,
    defaultFullscreen: settings.defaultFullscreen,
    markWatchedNext: settings.markWatchedNext,
    comfort: settings.comfort,
  }
}

export type StartupDestination = "home" | "calendar" | "last" | { library: string }

export interface ViewingSettings {
  spoilerProtection: boolean
  nextEpisode: "off" | "ask" | "auto"
  countdownSeconds: number
  episodeLimit: number
  audioLanguages: string[]
  subtitleLanguages: string[]
  preferOriginalAudio: boolean
  subtitleMode: "server" | "off" | "forced" | "always" | "foreignAudio"
  resumeRewindSeconds: number
  textScale: number
  posterSize: number
  previewDelayMs: number
  startupDestination: StartupDestination
  rememberFilters: boolean
  hideWatched: boolean
}

export type ViewingSettingsWrite = Omit<ViewingSettings, "startupDestination"> & {
  startupDestination?: StartupDestination
}

export function viewingSettingsWrite(settings: ViewingSettings, source: ViewingSettings | undefined): ViewingSettingsWrite {
  const { startupDestination, ...others } = settings
  return source && sameJson(startupDestination, source.startupDestination) ? others : settings
}

export interface PlayerComfort {
  subtitleSize: number
  subtitleOutline: number
  subtitleBackground: number
  subtitlePosition: number
  seekBackSeconds: number
  seekForwardSeconds: number
  pauseKey: string
  muteKey: string
  fullscreenKey: string
  seekBackKey: string
  seekForwardKey: string
  stopKey: string
  subtitlesKey: string
  seekBackThirtyKey: string
  seekForwardThirtyKey: string
}

export interface ClientSettings {
  client: {
    player: PlayerSettings
    playback: {
      streamingQuality: StreamingQualityId
      skipIntro: SegmentSkipMode
      skipCredits: SegmentSkipMode
      skipRecap: SegmentSkipMode
      skipCommercial: SegmentSkipMode
    }
    application: {
      closeBehavior: "exit_app" | "minimize_window"
      showScrollbars: boolean
      logLevel: "trace" | "debug" | "info" | "warn" | "error"
    }
  }
  appearance: AppearanceSettings
  capabilities: {
    platform: "windows" | "macos" | "linux" | "other"
    libmpv: boolean
    integratedLibmpvOverlay: boolean
    mpvInstaller: boolean
  }
  recoveries: { area: string; restoredBackup: boolean }[]
  serverUrl: string | null
}

type SegmentSkipMode = "hidden" | "disabled" | "prompt" | "always"

export interface AppearanceSettings {
  accent: "signal" | "cobalt" | "amber" | "violet"
  density: "compact" | "comfortable"
  artworkIntensity: number
  backdropIntensity: number
  reducedMotion: boolean
  cardPreviews: boolean
  showMediaInfo: boolean
  /** Canonical IDs from the fixed public MDBList source catalog. */
  ratingSources: string[]
}

export type StreamingQualityId =
  | "original"
  | "auto"
  | "120_mbps"
  | "80_mbps"
  | "60_mbps"
  | "40_mbps"
  | "20_mbps"
  | "10_mbps"
  | "5_mbps"
  | "3_mbps"
  | "1_5_mbps"

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
