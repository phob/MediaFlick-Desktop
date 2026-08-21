import type { ItemSummary } from "./api"

/**
 * Rail order, not server order: Jellyfin numbers Specials as season 0 so it
 * arrives first, but it is the season people reach for last.
 */
export function seasonRailOrder(seasons: ItemSummary[]) {
  return [
    ...seasons.filter((season) => season.indexNumber !== 0),
    ...seasons.filter((season) => season.indexNumber === 0),
  ]
}
