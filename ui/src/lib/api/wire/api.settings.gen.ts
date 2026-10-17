// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { Platform } from "./maintenance.player_setup.gen.ts"
import type { AppearanceAccent, AppearanceDensity, CloseBehavior, FullscreenBehavior, PlayerBackend, SegmentSkipMode, StreamingQuality } from "./preferences.model.gen.ts"
import type { AppearanceSettingsPatch, ApplicationSettingsPatch, PlaybackSettingsPatch, PlayerSettingsPatch } from "./preferences.service.gen.ts"
import type { NextEpisode, PlayerComfort, StartupView, SubtitleMode } from "./preferences.viewing.gen.ts"

export type AppearanceView = {
  accent: AppearanceAccent,
  density: AppearanceDensity,
  artworkIntensity: number,
  backdropIntensity: number,
  reducedMotion: boolean,
  cardPreviews: boolean,
  showMediaInfo: boolean,
  /**
   * Canonical IDs from the fixed public MDBList source catalog.
   */
  ratingSources: Array<string>,
};

export type ApplicationView = {
  closeBehavior: CloseBehavior,
  showScrollbars: boolean,
  logLevel: string,
};

export type BrowsingBody = {
  page: string,
  route: string,
};

export type BrowsingRoutes = { [key in string]: string };

export type BrowsingSaved = {
  saved: boolean,
};

export type Capabilities = {
  platform: Platform,
  libmpv: boolean,
  integratedLibmpvOverlay: boolean,
  mpvInstaller: boolean,
};

export type ClientView = {
  player: PlayerView,
  playback: PlaybackView,
  application: ApplicationView,
};

export type PlaybackView = {
  streamingQuality: StreamingQuality,
  skipIntro: SegmentSkipMode,
  skipCredits: SegmentSkipMode,
  skipRecap: SegmentSkipMode,
  skipCommercial: SegmentSkipMode,
};

export type PlayerView = {
  playerBackend: PlayerBackend,
  mpvPath: string | null,
  defaultFullscreen: FullscreenBehavior,
  markWatchedNext: string | null,
  comfort: PlayerComfort,
  playerConfigured: boolean,
};

export type Recovery = {
  area: string,
  restoredBackup: boolean,
};

export type SettingsView = {
  client: ClientView,
  appearance: AppearanceView,
  capabilities: Capabilities,
  recoveries: Array<Recovery>,
  serverUrl: string | null,
};

export type ViewingSettingsWrite = Omit<ViewingView, "startupDestination"> & { startupDestination?: StartupView };

export type ViewingView = {
  spoilerProtection: boolean,
  nextEpisode: NextEpisode,
  countdownSeconds: number,
  /**
   * Zero leaves continuous playback unlimited.
   */
  episodeLimit: number,
  audioLanguages: Array<string>,
  subtitleLanguages: Array<string>,
  preferOriginalAudio: boolean,
  subtitleMode: SubtitleMode,
  resumeRewindSeconds: number,
  textScale: number,
  posterSize: number,
  previewDelayMs: number,
  startupDestination: StartupView,
  rememberFilters: boolean,
  hideWatched: boolean,
};

export type ApiSettingsRoutes = {
  "GET /api/settings": { params: null, body: null, kind: "json", response: SettingsView },
  "GET /api/settings/viewing": { params: null, body: null, kind: "json", response: ViewingView },
  "PATCH /api/settings/viewing": { params: null, body: ViewingSettingsWrite, kind: "json", response: ViewingView },
  "GET /api/settings/browsing": { params: null, body: null, kind: "json", response: BrowsingRoutes },
  "PATCH /api/settings/browsing": { params: null, body: BrowsingBody, kind: "json", response: BrowsingSaved },
  "PATCH /api/settings/client/player": { params: null, body: PlayerSettingsPatch, kind: "json", response: SettingsView },
  "PATCH /api/settings/client/playback": { params: null, body: PlaybackSettingsPatch, kind: "json", response: SettingsView },
  "PATCH /api/settings/client/application": { params: null, body: ApplicationSettingsPatch, kind: "json", response: SettingsView },
  "PATCH /api/settings/appearance": { params: null, body: AppearanceSettingsPatch, kind: "json", response: SettingsView },
};
