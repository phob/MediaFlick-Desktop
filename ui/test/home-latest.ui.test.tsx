import { render, screen } from "@testing-library/react"
import { describe, expect, test } from "vitest"
import { queryKeys } from "@/lib/query-client"
import { itemSummary } from "./support/fixtures"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

import Home from "@/routes/Home"

const item = (id: string, name: string, kind: "Movie" | "Series") =>
  itemSummary({ id, name, kind })

function isoDate(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, "0")
  const day = String(date.getDate()).padStart(2, "0")
  return `${year}-${month}-${day}`
}

function dateFromToday(days: number) {
  const date = new Date()
  date.setDate(date.getDate() + days)
  return isoDate(date)
}

function renderHome(error: Error | null = null) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.home, {
    configuration: {
      billboard: true,
      watching: { continueWatching: true, nextUp: true, combine: true },
      elements: [
        { kind: "builtIn", id: "recentlyAdded", enabled: true, label: "Recently Added", available: true, category: "Built-in" },
        { kind: "builtIn", id: "upcoming", enabled: true, label: "Upcoming", available: true, category: "Built-in" },
        { kind: "builtIn", id: "latestMovies", enabled: true, label: "Latest Movies", available: true, category: "Built-in" },
        { kind: "builtIn", id: "latestShows", enabled: true, label: "Latest Shows", available: true, category: "Built-in" },
      ],
    },
    continueWatching: [],
    rows: [
      { kind: "builtIn", id: "recentlyAdded", title: "Recently Added", items: [item("recent", "Recent", "Movie"), item("recent-show", "Recent Series", "Series")] },
      { kind: "builtIn", id: "latestMovies", title: "Latest Movies", items: [item("movie", "Movie", "Movie")] },
      { kind: "builtIn", id: "latestShows", title: "Latest Series", items: [item("show", "Series", "Series")] },
    ],
  })
  client.setQueryData(queryKeys.homeResume, { continueWatching: [], nextUp: [] })
  client.setQueryData(queryKeys.billboard, { items: [] })
  client.setQueryData(queryKeys.items({ favorite: true, sort: "added", limit: 24 }), { items: [] })
  client.setQueryData(queryKeys.genres, { genres: [] })
  client.setQueryData(queryKeys.calendar(dateFromToday(0), dateFromToday(90)), {
    entries: [
      {
        kind: "episode",
        date: dateFromToday(4),
        dateKind: "air",
        title: "Season Premiere",
        seriesTitle: "Northstar",
        season: 2,
        episode: 1,
        tmdbId: 101,
        tvdbId: null,
        seriesTmdbId: 100,
        seriesTvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
        seriesLibraryItemId: "northstar",
      },
      {
        kind: "episode",
        date: dateFromToday(4),
        dateKind: "air",
        title: "Second Episode",
        seriesTitle: "Northstar",
        season: 2,
        episode: 2,
        tmdbId: 102,
        tvdbId: null,
        seriesTmdbId: 100,
        seriesTvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
        seriesLibraryItemId: "northstar",
      },
      {
        kind: "movie",
        date: dateFromToday(9),
        dateKind: "digital",
        title: "Digital Movie",
        seriesTitle: null,
        season: null,
        episode: null,
        tmdbId: 200,
        tvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
      },
      {
        kind: "episode",
        date: dateFromToday(11),
        dateKind: "air",
        title: "Third Episode",
        seriesTitle: "Northstar",
        season: 2,
        episode: 3,
        tmdbId: 103,
        tvdbId: null,
        seriesTmdbId: 100,
        seriesTvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
        seriesLibraryItemId: "northstar",
      },
      {
        kind: "movie",
        date: dateFromToday(10),
        dateKind: "cinema",
        title: "Cinema Movie",
        seriesTitle: null,
        season: null,
        episode: null,
        tmdbId: 201,
        tvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
      },
      {
        kind: "movie",
        date: dateFromToday(12),
        dateKind: "physical",
        title: "Physical Movie",
        seriesTitle: null,
        season: null,
        episode: null,
        tmdbId: 202,
        tvdbId: null,
        monitored: true,
        hasFile: false,
        posterUrl: null,
        libraryItemId: null,
      },
    ],
    refreshedAt: null,
    sources: {},
    windowStart: dateFromToday(0),
    windowEnd: dateFromToday(90),
    provider: "plugin",
  })
  if (error) {
    const query = client.getQueryCache().find({ queryKey: queryKeys.home })
    if (!query) throw new Error("Expected the seeded Home query")
    query.setState({ ...query.state, error, status: "error" })
  }
  return render(
    <TestProviders client={client}>
        <Home />
    </TestProviders>,
  )
}

describe("home latest shelves", () => {
  test("renders cached shelves while billboard and live Next Up are pending", () => {
    renderHome()

    expect(screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)).toEqual([
      "Recently Added",
      "Upcoming",
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

  test("shows season starts, later episodes, and every movie release channel in one upcoming shelf", () => {
    renderHome()

    const upcoming = screen.getByRole("heading", { name: "Upcoming" }).closest("section")
    expect(upcoming?.textContent).toContain("NEW SEASONNorthstarS02E01")
    expect(screen.getByText("NEW SEASON")).toBeTruthy()
    expect(screen.getByText("S02E01")).toBeTruthy()
    expect(upcoming?.textContent).toContain("Digital MovieDigital release")
    expect(upcoming?.textContent).toContain("Cinema MovieCinema release")
    expect(upcoming?.textContent).toContain("Physical MoviePhysical release")
    expect(upcoming?.textContent).toContain("Third EpisodeNorthstar · S02E03")
    expect(upcoming?.textContent).not.toContain("Second Episode")
    expect(upcoming?.querySelector('img[src*="northstar/Backdrop"]')).toBeTruthy()
    expect(upcoming?.querySelector("a")?.getAttribute("href")).toBe("/calendar")
  })

  test("keeps valid cached shelves visible when a background refresh fails", () => {
    renderHome(new Error("offline"))

    expect(screen.getByRole("heading", { name: "Recently Added" })).toBeTruthy()
    expect(screen.queryByText("offline")).toBeNull()
  })
})
