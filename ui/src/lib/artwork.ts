export const SQUARE_ART_MIN = 0.8
export const SQUARE_ART_MAX = 1.3

export type ArtworkShape = "portrait" | "square" | "landscape"

export function artworkShape(aspect: number | null): ArtworkShape {
  if (aspect == null || aspect < SQUARE_ART_MIN) return "portrait"
  return aspect <= SQUARE_ART_MAX ? "square" : "landscape"
}
