import { describe, expect, test } from "vitest"
import { backdropUrl, type ItemDetail } from "../src/lib/api"

function detailItem(overrides: Partial<ItemDetail>): ItemDetail {
  return {
    id: "item-1",
    kind: "Movie",
    name: "Example title",
    year: null,
    runtimeTicks: null,
    communityRating: null,
    officialRating: null,
    seriesId: null,
    seriesName: null,
    indexNumber: null,
    parentIndexNumber: null,
    primaryImageTag: null,
    thumbImageTag: null,
    logoImageTag: null,
    backdropImageTag: "backdrop-tag",
    childCount: null,
    premiereDate: null,
    seasonId: null,
    played: false,
    playCount: 0,
    positionTicks: 0,
    favorite: false,
    genres: [],
    originalTitle: null,
    providerIds: { tmdb: null, imdb: null, tvdb: null },
    parentId: null,
    dateCreated: null,
    ...overrides,
  }
}

/** The image owner and kind, independent of rendition size and cache tag. */
function imagePath(url: string | null) {
  return url === null ? null : new URL(url, "https://app.test").pathname
}

describe("shared detail hero", () => {
  test.each(["Season", "Episode"] as const)(
    "loads an inherited %s backdrop from its series image owner",
    (kind) => {
      const item = detailItem({ id: "child/id", kind, seriesId: "series/id" })

      expect(imagePath(backdropUrl(item))).toBe("/api/image/series%2Fid/Backdrop")
    },
  )

  test("keeps a movie backdrop on the movie item", () => {
    expect(imagePath(backdropUrl(detailItem({ id: "movie/id" })))).toBe("/api/image/movie%2Fid/Backdrop")
  })
})
