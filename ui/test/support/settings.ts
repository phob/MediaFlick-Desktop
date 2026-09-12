import type { ClientSettings } from "@/lib/api"
import { DEFAULT_COMFORT } from "@/lib/viewing"

export function clientSettingsFixture(): ClientSettings {
  return {
    client: {
      player: { playerBackend: "libmpv", mpvPath: null, mpchcPath: null, defaultFullscreen: "fullscreen", markWatchedNext: "w", playerConfigured: true },
      playback: { comfort: { ...DEFAULT_COMFORT }, streamingQuality: "original", skipIntro: "prompt", skipCredits: "prompt", skipRecap: "prompt", skipCommercial: "prompt" },
      application: { closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" },
    },
    appearance: { theme: "dark", accent: "signal", density: "comfortable", artworkIntensity: 100, backdropIntensity: 100, reducedMotion: false, cardPreviews: true, showMediaInfo: true, ratingSources: [] },
    capabilities: { platform: "windows", libmpv: true, mpchc: true, mpvInstaller: true },
    serverUrl: "https://jellyfin.example",
  }
}
