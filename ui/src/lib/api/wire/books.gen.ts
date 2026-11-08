// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type BookFormat =
  | "epub"
  | "mobi"
  | "azw"
  | "azw3"
  | "pdf"
  | "cbz"
  | "cbr"
  | "cb7"
  | "cbt";

export type BookResume =
  | { "kind": "start" }
  | { "kind": "exact", location: ReadingLocation }
  | { "kind": "fraction", fraction: Fraction }
  | { "kind": "page", page: number };

export type Cfi = string;

export type Fraction = number;

export type ReadingLocation =
  | { "kind": "text", cfi: Cfi, fraction: Fraction }
  | { "kind": "page", page: number, pages: number };

export type ReadingState =
  | { "state": "notStarted" }
  | { "state": "inProgress", percent: number | null }
  | { "state": "finished" };
