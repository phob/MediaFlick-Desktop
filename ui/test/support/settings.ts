import type { ClientSettings, PlayerComfort } from "@/lib/api"

export const DEFAULT_COMFORT: PlayerComfort = {
  subtitleSize: 100,
  subtitleOutline: 3,
  subtitleBackground: 0,
  subtitlePosition: 100,
  seekBackSeconds: 10,
  seekForwardSeconds: 30,
  pauseKey: "k",
  muteKey: "m",
  fullscreenKey: "f",
  seekBackKey: "j",
  seekForwardKey: "l",
  stopKey: "q",
  subtitlesKey: "v",
  seekBackThirtyKey: "DOWN",
  seekForwardThirtyKey: "UP",
}

export function clientSettingsFixture(): ClientSettings {
  const playerDefaults = {
    playerBackend: "libmpv" as const,
    mpvPath: null,
    defaultFullscreen: "fullscreen" as const,
    markWatchedNext: "w",
    comfort: { ...DEFAULT_COMFORT },
    audioPassthrough: false,
  }
  const playback = {
    streamingQuality: "original" as const,
    skipIntro: "prompt" as const,
    skipCredits: "prompt" as const,
    skipRecap: "prompt" as const,
    skipCommercial: "prompt" as const,
  }
  const application = {
    closeBehavior: "exit_app" as const,
    showScrollbars: false,
    logLevel: "debug",
  }
  const appearance = {
    accent: "signal" as const,
    density: "comfortable" as const,
    artworkIntensity: 100,
    backdropIntensity: 100,
    reducedMotion: false,
    cardPreviews: true,
    showMediaInfo: true,
    ratingSources: [],
  }
  return {
    client: {
      player: { ...playerDefaults, playerConfigured: true, comfort: { ...DEFAULT_COMFORT } },
      playback: { ...playback },
      application: { ...application },
    },
    appearance: { ...appearance, ratingSources: [] },
    defaults: {
      client: {
        player: { ...playerDefaults, comfort: { ...DEFAULT_COMFORT } },
        playback: { ...playback },
        application: { ...application },
      },
      appearance: { ...appearance, ratingSources: [] },
    },
    capabilities: { platform: "windows", libmpv: true, integratedLibmpvOverlay: false, mpvInstaller: true },
    recoveries: [],
    restartRequired: { logLevel: false, playerBackend: false },
    serverUrl: "https://jellyfin.example",
  }
}
