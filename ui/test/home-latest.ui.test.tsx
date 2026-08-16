import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { beforeEach, describe, expect, test, vi } from "vitest"
import type { ItemSummary } from "@/lib/api"

vi.mock("@/components/Billboard", () => ({ Billboard: () => null }))
vi.mock("@/components/MediaCard", () => ({
  MediaCard: ({ item }: { item: ItemSummary }) => <div>{item.name}</div>,
}))

const item = (id: string, name: string, kind: "Movie" | "Series") =>
  ({ id, name, kind }) as ItemSummary
const homeState = vi.hoisted(() => ({ error: null as Error | null }))

vi.mock("@/lib/queries", () => ({
  useHome: () => ({
    data: {
      rows: [
        { id: "recent", title: "Recently Added", items: [item("recent", "Recent", "Movie"), item("recent-show", "Recent Series", "Series")] },
        { id: "latest-movies", title: "Latest Movies", items: [item("movie", "Movie", "Movie")] },
        { id: "latest-shows", title: "Latest Series", items: [item("show", "Series", "Series")] },
      ],
    },
    error: homeState.error,
    isPending: false,
  }),
  useHomeResume: () => ({ data: undefined, isPending: true }),
  useBillboard: () => ({ data: undefined, isPending: true }),
  useItems: () => ({ data: { items: [] } }),
  useGenres: () => ({ data: { genres: [] } }),
  useItem: () => ({ data: undefined }),
}))

import Home from "@/routes/Home"

describe("home latest shelves", () => {
  beforeEach(() => {
    homeState.error = null
  })

  test("renders cached shelves while billboard and live Next Up are pending", () => {
    render(
      <MemoryRouter>
        <Home />
      </MemoryRouter>,
    )

    expect(screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)).toEqual([
      "Recently Added",
      "Latest Movies",
      "Latest Series",
    ])
    expect(
      screen.getByRole("heading", { name: "Recently Added" }).closest("section")?.querySelector("a")
        ?.getAttribute("href"),
    ).toBe("/library?kind=Movie,Series&sort=added")
    expect(
      screen.getByRole("heading", { name: "Latest Movies" }).closest("section")?.querySelector("a")
        ?.getAttribute("href"),
    ).toBe("/library?kind=Movie&sort=year")
    expect(
      screen.getByRole("heading", { name: "Latest Series" }).closest("section")?.querySelector("a")
        ?.getAttribute("href"),
    ).toBe("/library?kind=Series&sort=year")
  })

  test("keeps valid cached shelves visible when a background refresh fails", () => {
    homeState.error = new Error("offline")
    render(
      <MemoryRouter>
        <Home />
      </MemoryRouter>,
    )

    expect(screen.getByRole("heading", { name: "Recently Added" })).toBeTruthy()
    expect(screen.queryByText("offline")).toBeNull()
  })
})
