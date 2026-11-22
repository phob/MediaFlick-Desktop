import type { ItemSummary } from "./api"

export function seasonRailOrder(seasons: ItemSummary[]) {
  return [
    ...seasons.filter((season) => season.indexNumber !== 0),
    ...seasons.filter((season) => season.indexNumber === 0),
  ]
}
