import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { DetailHeroLayout } from "../src/components/detail/DetailHeroLayout"
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

describe("shared detail hero", () => {
  test("always renders the supplied return action as part of the shared shell", () => {
    render(
      <MemoryRouter>
        <DetailHeroLayout
          back={{ to: "/library?kind=Movie", label: "Back to library" }}
          title="Example title"
          facts={["Movie", "2026"]}
          genres={[{ label: "Drama" }]}
          overview={<p>Example overview</p>}
        />
      </MemoryRouter>,
    )

    expect(screen.getByTestId("detail-hero-layout")).toBeTruthy()
    expect(screen.getByRole("link", { name: "Back to library" }).getAttribute("href"))
      .toBe("/library?kind=Movie")
    expect(screen.getByRole("heading", { name: "Example title" })).toBeTruthy()
    expect(screen.getByText("Example overview")).toBeTruthy()
  })

  test.each(["Season", "Episode"] as const)(
    "loads an inherited %s backdrop from its series image owner",
    (kind) => {
      const item = detailItem({ id: "child/id", kind, seriesId: "series/id" })

      expect(backdropUrl(item)).toBe(
        "/api/image/series%2Fid/Backdrop?maxWidth=1920&tag=backdrop-tag",
      )
    },
  )

  test("keeps a movie backdrop on the movie item", () => {
    expect(backdropUrl(detailItem({ id: "movie/id" }))).toBe(
      "/api/image/movie%2Fid/Backdrop?maxWidth=1920&tag=backdrop-tag",
    )
  })
})
