// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { AppearanceAccent, AppearanceDensity, CloseBehavior, FullscreenBehavior, PlayerBackend, SegmentSkipMode, StreamingQuality } from "./preferences.model.gen.ts"
import type { PlayerComfort } from "./preferences.viewing.gen.ts"

export type AppearanceSettingsPatch = {
  accent: AppearanceAccent | null,
  density: AppearanceDensity | null,
  artworkIntensity: number | null,
  backdropIntensity: number | null,
  reducedMotion: boolean | null,
  cardPreviews: boolean | null,
  showMediaInfo: boolean | null,
  ratingSources: Array<string> | null,
};

export type ApplicationSettingsPatch = {
  closeBehavior: CloseBehavior | null,
  showScrollbars: boolean | null,
  logLevel: string | null,
};

export type PlaybackSettingsPatch = {
  streamingQuality: StreamingQuality | null,
  skipIntro: SegmentSkipMode | null,
  skipCredits: SegmentSkipMode | null,
  skipRecap: SegmentSkipMode | null,
  skipCommercial: SegmentSkipMode | null,
};

export type PlayerSettingsPatch = {
  comfort: PlayerComfort | null,
  playerBackend: PlayerBackend | null,
  mpvPath: string | null,
  defaultFullscreen: FullscreenBehavior | null,
  markWatchedNext: string | null,
};
