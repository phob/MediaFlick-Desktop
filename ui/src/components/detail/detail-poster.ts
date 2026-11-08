import type { HeroArtwork, HeroArtworkAspect } from "@/components/detail/DetailHeroLayout"
import { DETAIL_POSTER_WIDTH, imageUrl, type ItemSummary } from "@/lib/api"
import { artworkShape, type ArtworkShape } from "@/lib/artwork"

const HERO_FRAME: Record<ArtworkShape, HeroArtworkAspect> = {
  portrait: "poster",
  square: "square",
  landscape: "still",
}

export function detailPoster(
  item: Pick<ItemSummary, "id" | "primaryImageTag" | "primaryAspect">,
  progress?: number,
): HeroArtwork | null {
  if (!item.primaryImageTag) return null
  return {
    src: imageUrl(item, "Primary", DETAIL_POSTER_WIDTH),
    aspect: HERO_FRAME[artworkShape(item.primaryAspect)],
    fit: "contain",
    progress,
  }
}
