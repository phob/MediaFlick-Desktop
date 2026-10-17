// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type SegmentType =
  | "intro"
  | "outro"
  | "recap"
  | "commercial";

export type SkipSegment = {
  segmentType: SegmentType,
  startTicks: number,
  endTicks: number,
  triggered: boolean,
};
