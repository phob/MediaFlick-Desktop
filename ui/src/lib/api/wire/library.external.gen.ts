// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type ExternalLink =
  | { "kind": "provider", id: ExternalProvider, label: string }
  | { "kind": "url", id: ExternalLinkId, label: string, href: string, actionLabel?: string };

export type ExternalLinkId =
  | "imdb"
  | "tmdb"
  | "tvdb"
  | "letterboxd"
  | "trakt"
  | "rotten-tomatoes-search";

export type ExternalProvider =
  | "imdb"
  | "tmdb"
  | "tvdb"
  | "letterboxd"
  | "trakt";
