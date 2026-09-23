import type { AppearanceSettings, ClientSettings, PlayerSettings } from "@/lib/api"
import { DEFAULT_COMFORT } from "@/lib/viewing"

/**
 * The values each shelf's Reset restores. They mirror the shell's defaults;
 * Reset only fills the draft, so nothing changes until Save.
 */
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

export const DEFAULT_MARK_WATCHED_NEXT = "w"

/**
 * Player defaults depend on the device: the built-in player where this build
 * ships libmpv, external mpv otherwise. `playerConfigured` is computed by the
 * shell, so the current value is kept.
 */
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
