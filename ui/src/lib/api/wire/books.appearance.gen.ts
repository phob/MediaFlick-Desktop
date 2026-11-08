// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type FontSize = number;

export type LineSpacing =
  | "compact"
  | "normal"
  | "relaxed";

export type Margins =
  | "narrow"
  | "normal"
  | "wide";

export type PageFit =
  | "page"
  | "width";

export type ReaderAppearance = {
  fontSize: FontSize,
  lineSpacing: LineSpacing,
  margins: Margins,
  theme: ReaderTheme,
  flow: ReaderFlow,
  fit: PageFit,
};

export type ReaderFlow =
  | "paginated"
  | "scrolled";

export type ReaderTheme =
  | "app"
  | "light"
  | "sepia"
  | "dark";
