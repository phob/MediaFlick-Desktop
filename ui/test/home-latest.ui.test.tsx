import { render, screen, within } from "@testing-library/react"
import { describe, expect, test } from "vitest"
import { queryKeys } from "@/lib/query-client"
import { itemSummary, requireElement } from "./support/fixtures"
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
        { kind: "builtIn", id: "recentlyAdded", enabled: true, label: "Recently Added Movies", available: true, category: "Built-in" },
        { kind: "builtIn", id: "recentlyAddedShows", enabled: true, label: "Recently Added Shows", available: true, category: "Built-in" },
        { kind: "builtIn", id: "upcoming", enabled: true, label: "Release Timeline", available: true, category: "Built-in" },
        { kind: "builtIn", id: "latestMovies", enabled: true, label: "Latest Movies", available: true, category: "Built-in" },
        { kind: "builtIn", id: "latestShows", enabled: true, label: "Latest Shows", available: true, category: "Built-in" },
      ],
    },
    continueWatching: [],
    rows: [
      { kind: "builtIn", id: "recentlyAdded", title: "Recently Added Movies", items: [item("recent", "Recent", "Movie")] },
      {
        kind: "builtIn",
        id: "recentlyAddedShows",
        title: "Recently Added Shows",
        items: [itemSummary({ id: "new-episode", name: "Half Loop", kind: "Episode", seriesId: "sev", seriesName: "Severance", parentIndexNumber: 1, indexNumber: 2 })],
      },
      { kind: "builtIn", id: "latestMovies", title: "Latest Movies", items: [item("movie", "Movie", "Movie")] },
      { kind: "builtIn", id: "latestShows", title: "Latest Series", items: [item("show", "Series", "Series")] },
    ],
  })
  client.setQueryData(queryKeys.homeResume, { continueWatching: [], nextUp: [] })
  client.setQueryData(queryKeys.billboard, { items: [] })
  client.setQueryData(queryKeys.items({ favorite: true, sort: "added", limit: 24 }), { items: [] })
  client.setQueryData(queryKeys.genres, { genres: [] })
  client.setQueryData(queryKeys.calendar(dateFromToday(-30), dateFromToday(90)), {
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
        posterPath: null,
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
        posterPath: null,
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
        posterPath: null,
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
        posterPath: null,
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
        posterPath: null,
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
        posterPath: null,
        libraryItemId: null,
      },
    ],
    refreshedAt: null,
    sources: {},
    windowStart: dateFromToday(-30),
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

function shelf(title: string) {
  const section = screen.getByRole("heading", { name: title }).closest("section")
  return within(requireElement(section, `${title} shelf`))
}

describe("home latest shelves", () => {
  test("renders cached shelves in order with links to their full library views", () => {
    renderHome()

    expect(screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)).toEqual([
      "Recently Added Movies",
      "Recently Added Shows",
      "Release Timeline",
      "Latest Movies",
      "Latest Series",
    ])
    expect(shelf("Recently Added Movies").getByRole("link", { name: "All" }).getAttribute("href"))
      .toBe("/library?kind=Movie&sort=added")
    expect(shelf("Latest Movies").getByRole("link", { name: "All" }).getAttribute("href"))
      .toBe("/library?kind=Movie&sort=year")
    expect(shelf("Latest Series").getByRole("link", { name: "All" }).getAttribute("href"))
      .toBe("/library?kind=Series&sort=year")
  })

  test("a newly added episode in Recently Added Shows links to its item page", () => {
    renderHome()

    const episode = requireElement(
      screen.getByRole("heading", { name: "Recently Added Shows" }).closest("section")?.querySelector("article") ?? null,
      "recently added episode card",
    )
    expect(episode.querySelector("a")?.getAttribute("href")).toBe("/item/new-episode")
  })

  test("shows season starts, later episodes, and every movie release channel in one upcoming shelf", () => {
    renderHome()

    const upcoming = shelf("Release Timeline")
    // A season premiere is one card for the series; same-day later episodes fold into it.
    expect(upcoming.getByRole("link", { name: "Open Northstar" })).toBeTruthy()
    expect(upcoming.getByText("NEW SEASON")).toBeTruthy()
    expect(upcoming.queryByText("Second Episode")).toBeNull()
    // Episode names stay concealed while spoiler protection is on.
    expect(upcoming.queryByText("Third Episode")).toBeNull()
    expect(upcoming.getByText("S02E03")).toBeTruthy()
    for (const channel of ["Digital release", "Cinema release", "Physical release"]) {
      expect(upcoming.getByText(channel)).toBeTruthy()
    }
    expect(upcoming.getByRole("link", { name: "All" }).getAttribute("href")).toBe("/calendar")
  })

  test("keeps valid cached shelves visible when a background refresh fails", () => {
    renderHome(new Error("offline"))

    expect(screen.getByRole("heading", { name: "Recently Added Movies" })).toBeTruthy()
    expect(screen.queryByText("offline")).toBeNull()
  })
})
