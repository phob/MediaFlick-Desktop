import type { ReaderAppearance } from "@/lib/api"

type Theme = ReaderAppearance["theme"]

const TOKENS: Readonly<Record<Theme, { paper: string; ink: string; link: string; scheme: "light" | "dark" }>> = {
  app: { paper: "--background", ink: "--foreground", link: "--primary", scheme: "dark" },
  light: { paper: "--reader-light-paper", ink: "--reader-light-ink", link: "--reader-light-link", scheme: "light" },
  sepia: { paper: "--reader-sepia-paper", ink: "--reader-sepia-ink", link: "--reader-sepia-link", scheme: "light" },
  dark: { paper: "--reader-dark-paper", ink: "--reader-dark-ink", link: "--reader-dark-link", scheme: "dark" },
}

const LINE_HEIGHT: Readonly<Record<ReaderAppearance["lineSpacing"], number>> = { compact: 1.25, normal: 1.5, relaxed: 1.8 }

/** Column widths in em of the base size, so a column holds as many words on any screen. */
const MARGINS: Readonly<Record<ReaderAppearance["margins"], { gap: string; columnEm: number }>> = {
  narrow: { gap: "4%", columnEm: 40 },
  normal: { gap: "6%", columnEm: 34 },
  wide: { gap: "10%", columnEm: 28 },
}

/** A 1080p screen minus the reader's footer, where 100% text is 24px. */
const REFERENCE = { width: 1920, height: 1048, basePx: 24 } as const
const MIN_BASE_PX = 16

export const FONT_SIZE = { min: 80, max: 200, step: 10 } as const

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

/** The text size at 100% for a reader of this CSS size, rounded to half a pixel; it grows with the screen, as viewing distance does. */
export function basePx(width: number, height: number) {
  const scale = Math.min(width / REFERENCE.width, height / REFERENCE.height)
  return Math.max(MIN_BASE_PX, Math.round(REFERENCE.basePx * scale * 2) / 2)
}

export function bookStyles(appearance: ReaderAppearance, base: number) {
  const { paper, ink, link, scheme } = themeColors(appearance.theme)
  return `
    html { color-scheme: ${scheme} !important; font-size: ${(base * appearance.fontSize) / 100}px !important; }
    html, body { background: ${paper} !important; color: ${ink} !important; }
    body *:not(a) { color: inherit !important; background-color: transparent !important; }
    a, a:link, a:visited { color: ${link} !important; }
    p, li, blockquote, dd, div { line-height: ${LINE_HEIGHT[appearance.lineSpacing]} !important; }
  `
}

export function layoutAttributes(appearance: ReaderAppearance, base: number) {
  const margins = MARGINS[appearance.margins]
  // foliate reads these with parseFloat, so they must be pixels, not em.
  return { flow: appearance.flow, gap: margins.gap, "max-inline-size": `${margins.columnEm * base}px`, margin: `${2 * base}px` }
}

export function fitAttribute(appearance: ReaderAppearance) {
  return appearance.fit === "width" ? "fit-width" : "fit-page"
}
