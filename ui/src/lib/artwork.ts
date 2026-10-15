/** Art at least this wide for its height is near square: an album cover or a photo. */
export const SQUARE_ART_MIN = 0.8
/** Art wider than this is landscape: a 4:3 or 16:9 still. */
export const SQUARE_ART_MAX = 1.3

export type ArtworkShape = "portrait" | "square" | "landscape"

/** The shape of primary art from its width over height. Art of unknown shape is a poster. */
export function artworkShape(aspect: number | null): ArtworkShape {
  if (aspect == null || aspect < SQUARE_ART_MIN) return "portrait"
  return aspect <= SQUARE_ART_MAX ? "square" : "landscape"
}
