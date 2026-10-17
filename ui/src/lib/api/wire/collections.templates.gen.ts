// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

import type { CollectionSource, MediaType, RefreshCadence, ResultLimit } from "./collections.model.gen.ts"

export type CollectionTemplate = {
  title: string,
  description: string,
  category: TemplateCategory,
  pictogram: TemplatePictogram,
  source: CollectionSource,
  mediaType: MediaType,
  limit: ResultLimit,
  cadence: RefreshCadence,
  id: string,
};

export type TemplateCategory =
  | "trending"
  | "popular"
  | "streamingServices"
  | "topRated"
  | "inTheaters"
  | "upcoming"
  | "onAir"
  | "editorial"
  | "custom";

export type TemplatePictogram =
  | "award"
  | "binary"
  | "blocks"
  | "bone"
  | "bookOpen"
  | "briefcase"
  | "bug"
  | "calendarClock"
  | "calendarDays"
  | "circle"
  | "compass"
  | "crosshair"
  | "drama"
  | "film"
  | "flame"
  | "ghost"
  | "heart"
  | "landmark"
  | "languages"
  | "laugh"
  | "listVideo"
  | "monitorPlay"
  | "mountain"
  | "music"
  | "orbit"
  | "palette"
  | "pawPrint"
  | "popcorn"
  | "rocket"
  | "search"
  | "slidersHorizontal"
  | "sparkles"
  | "star"
  | "swords"
  | "telescope"
  | "trendingUp"
  | "trophy"
  | "tv"
  | "usersRound"
  | "wandSparkles"
  | "zap";
