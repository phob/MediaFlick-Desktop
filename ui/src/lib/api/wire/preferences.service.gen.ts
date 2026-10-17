// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { AppearanceAccent, AppearanceDensity, CloseBehavior, FullscreenBehavior, PlayerBackend, SegmentSkipMode, StreamingQuality } from "./preferences.model.gen.ts"
import type { PlayerComfort } from "./preferences.viewing.gen.ts"

export type AppearanceSettingsPatch = {
  accent?: AppearanceAccent,
  density?: AppearanceDensity,
  artworkIntensity?: number,
  backdropIntensity?: number,
  reducedMotion?: boolean,
  cardPreviews?: boolean,
  showMediaInfo?: boolean,
  ratingSources?: Array<string>,
};

export type ApplicationSettingsPatch = {
  closeBehavior?: CloseBehavior,
  showScrollbars?: boolean,
  logLevel?: string,
};

export type PlaybackSettingsPatch = {
  streamingQuality?: StreamingQuality,
  skipIntro?: SegmentSkipMode,
  skipCredits?: SegmentSkipMode,
  skipRecap?: SegmentSkipMode,
  skipCommercial?: SegmentSkipMode,
};

export type PlayerSettingsPatch = {
  comfort?: PlayerComfort,
  playerBackend?: PlayerBackend,
  mpvPath?: string | null,
  defaultFullscreen?: FullscreenBehavior,
  markWatchedNext?: string | null,
};
