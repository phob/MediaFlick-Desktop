import type { ReaderAppearance } from "@/lib/api"

type Theme = ReaderAppearance["theme"]

const TOKENS: Readonly<Record<Theme, { paper: string; ink: string; link: string; scheme: "light" | "dark" }>> = {
  app: { paper: "--background", ink: "--foreground", link: "--primary", scheme: "dark" },
  light: { paper: "--reader-light-paper", ink: "--reader-light-ink", link: "--reader-light-link", scheme: "light" },
  sepia: { paper: "--reader-sepia-paper", ink: "--reader-sepia-ink", link: "--reader-sepia-link", scheme: "light" },
  dark: { paper: "--reader-dark-paper", ink: "--reader-dark-ink", link: "--reader-dark-link", scheme: "dark" },
}

const LINE_HEIGHT: Readonly<Record<ReaderAppearance["lineSpacing"], number>> = { compact: 1.25, normal: 1.5, relaxed: 1.8 }

/** The paginator's layout attributes per margin choice: the gap between columns and the widest column. */
const MARGINS: Readonly<Record<ReaderAppearance["margins"], { gap: string; maxInlineSize: string }>> = {
  narrow: { gap: "4%", maxInlineSize: "960px" },
  normal: { gap: "6%", maxInlineSize: "720px" },
  wide: { gap: "10%", maxInlineSize: "560px" },
}

export const FONT_SIZE = { min: 80, max: 200, step: 10 } as const

/** The colours of a reader theme, read from the app's tokens in `app.css`. */
export function themeColors(theme: Theme) {
  const style = getComputedStyle(document.documentElement)
  const tokens = TOKENS[theme]
  return {
    paper: style.getPropertyValue(tokens.paper).trim(),
    ink: style.getPropertyValue(tokens.ink).trim(),
    link: style.getPropertyValue(tokens.link).trim(),
    scheme: tokens.scheme,
  }
}

/**
 * The stylesheet foliate puts into each section of a reflowable book. Every
 * theme sets the page's colours over the book's own, so a book that prints
 * black text stays readable on the dark themes.
 */
export function bookStyles(appearance: ReaderAppearance) {
  const { paper, ink, link, scheme } = themeColors(appearance.theme)
  return `
    html { color-scheme: ${scheme} !important; font-size: ${appearance.fontSize}% !important; }
    html, body { background: ${paper} !important; color: ${ink} !important; }
    body *:not(a) { color: inherit !important; background-color: transparent !important; }
    a, a:link, a:visited { color: ${link} !important; }
    p, li, blockquote, dd, div { line-height: ${LINE_HEIGHT[appearance.lineSpacing]} !important; }
  `
}

export function layoutAttributes(appearance: ReaderAppearance) {
  const margins = MARGINS[appearance.margins]
  return { flow: appearance.flow, gap: margins.gap, "max-inline-size": margins.maxInlineSize, margin: "40px" }
}

export function fitAttribute(appearance: ReaderAppearance) {
  return appearance.fit === "width" ? "fit-width" : "fit-page"
}
