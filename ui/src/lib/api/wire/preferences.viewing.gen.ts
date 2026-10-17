// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { LibraryId } from "./library.libraries.gen.ts"

export type NextEpisode =
  | "off"
  | "ask"
  | "auto";

export type PlayerComfort = {
  subtitleSize: number,
  subtitleOutline: number,
  subtitleBackground: number,
  subtitlePosition: number,
  seekBackSeconds: number,
  seekForwardSeconds: number,
  pauseKey: string,
  muteKey: string,
  fullscreenKey: string,
  seekBackKey: string,
  seekForwardKey: string,
  stopKey: string,
  subtitlesKey: string,
  seekBackThirtyKey: string,
  seekForwardThirtyKey: string,
};

export type StartupView =
  | "home"
  | "calendar"
  | "last"
  | { "library": LibraryId };

export type SubtitleMode =
  | "server"
  | "off"
  | "forced"
  | "always"
  | "foreignAudio";
