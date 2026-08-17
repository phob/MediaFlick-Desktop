import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { queryKeys } from "@/lib/query-client"
import { itemSummary } from "./support/fixtures"

import Home from "@/routes/Home"

const item = (id: string, name: string, kind: "Movie" | "Series") =>
  itemSummary({ id, name, kind })

function renderHome(error: Error | null = null) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  client.setQueryData(queryKeys.home, {
    rows: [
      { id: "recent", title: "Recently Added", items: [item("recent", "Recent", "Movie"), item("recent-show", "Recent Series", "Series")] },
      { id: "latest-movies", title: "Latest Movies", items: [item("movie", "Movie", "Movie")] },
      { id: "latest-shows", title: "Latest Series", items: [item("show", "Series", "Series")] },
    ],
  })
  client.setQueryData(queryKeys.homeResume, { items: [] })
  client.setQueryData(queryKeys.billboard, { items: [] })
  client.setQueryData(queryKeys.items({ favorite: true, sort: "added", limit: 24 }), { items: [] })
  client.setQueryData(queryKeys.genres, { genres: [] })
  if (error) {
    const query = client.getQueryCache().find({ queryKey: queryKeys.home })
    if (!query) throw new Error("Expected the seeded Home query")
    query.setState({ ...query.state, error, status: "error" })
  }
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <Home />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe("home latest shelves", () => {
  test("renders cached shelves while billboard and live Next Up are pending", () => {
    renderHome()

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
    renderHome(new Error("offline"))

    expect(screen.getByRole("heading", { name: "Recently Added" })).toBeTruthy()
    expect(screen.queryByText("offline")).toBeNull()
  })
})
