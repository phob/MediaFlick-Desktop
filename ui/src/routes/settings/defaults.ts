import type { AppearanceSettings, ClientSettings, PlayerSettings } from "@/lib/api"
import { DEFAULT_COMFORT } from "@/lib/viewing"

export const DEFAULT_PLAYBACK_SETTINGS: ClientSettings["client"]["playback"] = {
  streamingQuality: "original",
  skipIntro: "prompt",
  skipCredits: "prompt",
  skipRecap: "prompt",
  skipCommercial: "prompt",
}

export const DEFAULT_APPLICATION_SETTINGS: ClientSettings["client"]["application"] = {
  closeBehavior: "exit_app",
  showScrollbars: false,
  logLevel: "debug",
}

export const DEFAULT_APPEARANCE: AppearanceSettings = {
  accent: "signal",
  density: "comfortable",
  artworkIntensity: 100,
  backdropIntensity: 100,
  reducedMotion: false,
  cardPreviews: true,
  showMediaInfo: true,
  ratingSources: [],
}

const DEFAULT_MARK_WATCHED_NEXT = "w"

export function defaultPlayerSettings(current: PlayerSettings, libmpvAvailable: boolean): PlayerSettings {
  return {
    ...current,
    playerBackend: libmpvAvailable ? "libmpv" : "mpv",
    mpvPath: null,
    defaultFullscreen: "fullscreen",
    markWatchedNext: DEFAULT_MARK_WATCHED_NEXT,
    comfort: { ...DEFAULT_COMFORT },
  }
}
