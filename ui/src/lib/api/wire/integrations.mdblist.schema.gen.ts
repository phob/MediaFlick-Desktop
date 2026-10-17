// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type NormalizedRating = {
  sourceId: string,
  value: number,
  /**
   * The source's 0 to 100 score, when it reports one.
   */
  score: number | null,
  votes: number | null,
  scaleMax: number,
};

export type RatingFormat =
  | "percent"
  | "decimal"
  | "integer"
  | "stars";

export type RatingSourceDefinition = {
  id: string,
  label: string,
  shortLabel: string,
  scaleMax: number,
  format: RatingFormat,
  known: boolean,
};
