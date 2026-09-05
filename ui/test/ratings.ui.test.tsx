import { DEFAULT_COMFORT } from "@/lib/viewing"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { describe, expect, test, vi } from "vitest"
import { DetailRatingReadout, RatingOverlayView } from "../src/components/RatingOverlay"
import type {
  ClientSettings,
  ItemRatings,
  ItemSummary,
  NormalizedRating,
  RatingSourceDefinition,
  RatingsIntegrationStatus,
} from "../src/lib/api"
import {
  formatRating,
  RatingsContext,
  useCardRatings,
  type DisplayRating,
} from "../src/lib/rating-context"
import { RatingsProvider } from "../src/lib/ratings"
import Settings, {
  Appearance,
  AppearanceSync,
  RatingSourceSelector,
} from "../src/routes/Settings"
import { queryKeys } from "../src/lib/query-client"
import { requireElement } from "./support/fixtures"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const definitions: RatingSourceDefinition[] = [
  { id: "imdb", label: "IMDb", shortLabel: "IMDb", scaleMax: 10, format: "decimal", known: true },
  { id: "letterboxd", label: "Letterboxd", shortLabel: "LB", scaleMax: 5, format: "stars", known: true },
  { id: "tomatoes", label: "Rotten Tomatoes Critics", shortLabel: "RT", scaleMax: 100, format: "percent", known: true },
  { id: "popcorn", label: "Rotten Tomatoes Audience", shortLabel: "RT A", scaleMax: 100, format: "percent", known: true },
  { id: "metacriticuser", label: "Metacritic Users", shortLabel: "MC U", scaleMax: 10, format: "decimal", known: true },
]

function display(sourceId: string, value: number): DisplayRating {
  const definition = definitions.find((candidate) => candidate.id === sourceId)
  if (!definition) throw new Error(`Expected rating source ${sourceId}`)
  const rating: NormalizedRating = {
    sourceId,
    rawSource: sourceId,
    value,
    score: null,
    votes: null,
    scaleMax: definition.scaleMax,
  }
  return { rating, definition, ...formatRating(rating, definition) }
}

const integrationStatus: RatingsIntegrationStatus = {
  boundaryVersion: 1,
  effectiveOrigin: "plugin",
  available: true,
  selectionEnabled: true,
  plugin: { available: true, capability: "ratings-v1", boundaryVersion: 1, detail: "Available" },
  sources: definitions,
  selectedSources: ["letterboxd"],
}

const clientSettings: ClientSettings = {
  client: {
    player: { playerBackend: "mpv", mpvPath: null, mpchcPath: null, defaultFullscreen: "fullscreen", markWatchedNext: "w", playerConfigured: false },
    playback: { comfort: DEFAULT_COMFORT, streamingQuality: "original", skipIntro: "prompt", skipCredits: "prompt", skipRecap: "prompt", skipCommercial: "prompt" },
    application: { closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" },
  },
  appearance: { theme: "system", accent: "signal", density: "comfortable", artworkIntensity: 100, backdropIntensity: 100, reducedMotion: false, cardPreviews: true, showMediaInfo: true, ratingSources: ["letterboxd"] },
  capabilities: { platform: "windows", libmpv: true, mpchc: true, mpvInstaller: true },
  serverUrl: null,
}

const item: ItemRatings = {
  id: "movie-1",
  ratings: [],
  origin: "plugin",
  fetchedAt: 1,
  sourceUpdatedAt: "2026-08-04T20:00:00Z",
  stale: false,
  schemaVersion: 1,
}

function RatingProbe({ id }: { id: string }) {
  const { item: ratingItem } = useCardRatings(id)
  return <span>{ratingItem ? `${id}:${ratingItem.ratings.length}` : `${id}:ready`}</span>
}

describe("configurable card ratings", () => {
  test("formats each native source scale without conflating RT critics and audience", () => {
    expect(display("letterboxd", 4.25).formatted).toBe("★4.3")
    expect(display("imdb", 8.1).accessibleValue).toBe("8.1 out of 10")
    expect(display("tomatoes", 97).formatted).toBe("97%")
    expect(display("popcorn", 91).formatted).toBe("91%")
  })

  test("renders multiple available ratings as one accessible definition list", () => {
    render(
      <RatingOverlayView
        itemName="The Matrix"
        ratingItem={item}
        ratings={[display("letterboxd", 4.2), display("tomatoes", 88), display("popcorn", 94)]}
      />,
    )

    const overlay = screen.getByLabelText("Ratings for The Matrix")
    expect(overlay.tagName).toBe("DL")
    expect(overlay.querySelectorAll("[data-rating-source-icon]")).toHaveLength(3)
    expect(overlay.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()
    expect(overlay.querySelector("[data-rating-source-icon='tomatoes']")).toBeTruthy()
    expect(overlay.querySelector("[data-rating-source-icon='popcorn']")).toBeTruthy()
    expect(overlay.textContent).not.toContain("LB")
    expect(overlay.textContent).not.toContain("RT A")
    expect(overlay.getAttribute("data-rating-origin")).toBe("plugin")
    expect(screen.getByLabelText("Letterboxd rating 4.2 out of 5")).toBeTruthy()
    expect(screen.getByLabelText("Rotten Tomatoes Critics rating 88 percent")).toBeTruthy()
    expect(screen.getByLabelText("Rotten Tomatoes Audience rating 94 percent")).toBeTruthy()
    expect(overlay.querySelector("[title*='via MDBList']")).toBeTruthy()
  })

  test("renders the selected MDBList sources in title details", () => {
    const letterboxd = display("letterboxd", 4.2).rating
    const register = vi.fn(() => vi.fn())
    render(
      <RatingsContext.Provider value={{
        items: new Map([[item.id, { ...item, ratings: [letterboxd] }]]),
        selected: ["letterboxd"],
        definitions: new Map(definitions.map((definition) => [definition.id, definition])),
        register,
      }}>
        <DetailRatingReadout item={{ id: item.id, name: "The Matrix" }} />
      </RatingsContext.Provider>,
    )

    const readout = screen.getByLabelText("Ratings for The Matrix")
    expect(screen.getByLabelText("Letterboxd rating 4.2 out of 5")).toBeTruthy()
    expect(readout.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()
    expect(readout.textContent).not.toContain("LB")
    expect(register).toHaveBeenCalledWith(item.id)
  })

  test("renders no placeholder when selected sources have no value", () => {
    const { container } = render(
      <RatingOverlayView itemName="Missing" ratingItem={item} ratings={[]} />,
    )
    expect(container.firstChild).toBeNull()
  })

  test("mounted cards coalesce and deduplicate without blocking their first render", async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      const path = String(input)
      if (path.endsWith("/api/integrations/ratings")) {
        return new Response(JSON.stringify(integrationStatus), { status: 200 })
      }
      return new Response(JSON.stringify({
        available: true,
        effectiveOrigin: "plugin",
        retryAt: null,
        diagnostic: null,
        items: [{ ...item, ratings: [{ sourceId: "letterboxd", rawSource: "letterboxd", value: 4.2, score: 84, votes: 10, scaleMax: 5 }] }],
      }), { status: 200 })
    })
    vi.stubGlobal("fetch", fetchMock)
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    client.setQueryData(queryKeys.status, { authenticated: true })
    client.setQueryData(queryKeys.ratingsStatus, integrationStatus)
    try {
      render(
        <QueryClientProvider client={client}>
          <RatingsProvider>
            <RatingProbe id="movie-1" />
            <RatingProbe id="movie-1" />
          </RatingsProvider>
        </QueryClientProvider>,
      )
      // Cards are committed before the post-render ratings timer starts.
      expect(screen.getAllByText("movie-1:ready")).toHaveLength(2)
      expect(fetchMock).not.toHaveBeenCalled()
      await act(async () => {
        vi.advanceTimersByTime(80)
        await Promise.resolve()
        await Promise.resolve()
      })
      const batchCalls = fetchMock.mock.calls.filter(([path]) => String(path).endsWith("/api/ratings/batch"))
      expect(batchCalls).toHaveLength(1)
      expect(JSON.parse(String(batchCalls[0][1]?.body))).toEqual({ ids: ["movie-1"] })
      expect(screen.getAllByText("movie-1:1")).toHaveLength(2)
    } finally {
      vi.useRealTimers()
      vi.unstubAllGlobals()
    }
  })

  test("a settling batch leaves other in-flight batches running", async () => {
    vi.useFakeTimers()
    type PendingBatch = {
      ids: string[]
      signal: AbortSignal | null | undefined
      resolve: (items: ItemRatings[]) => void
    }
    const pending: PendingBatch[] = []
    const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith("/api/integrations/ratings")) {
        return Promise.resolve(new Response(JSON.stringify(integrationStatus), { status: 200 }))
      }
      // SAFETY: the provider under test always posts a JSON `{ ids }` body.
      const { ids } = JSON.parse(String(init?.body)) as { ids: string[] }
      return new Promise<Response>((resolve, reject) => {
        init?.signal?.addEventListener("abort", () =>
          reject(new DOMException("aborted", "AbortError")))
        pending.push({
          ids,
          signal: init?.signal,
          resolve: (ratedItems) => resolve(new Response(JSON.stringify({
            available: true,
            effectiveOrigin: "plugin",
            retryAt: null,
            diagnostic: null,
            items: ratedItems,
          }), { status: 200 })),
        })
      })
    })
    vi.stubGlobal("fetch", fetchMock)
    const rated = (id: string): ItemRatings => ({
      ...item,
      id,
      ratings: [{ sourceId: "letterboxd", rawSource: "letterboxd", value: 4.2, score: 84, votes: 10, scaleMax: 5 }],
    })
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    client.setQueryData(queryKeys.ratingsStatus, integrationStatus)
    const harness = (ids: string[]) => (
      <QueryClientProvider client={client}>
        <RatingsProvider>
          {ids.map((id) => <RatingProbe key={id} id={id} />)}
        </RatingsProvider>
      </QueryClientProvider>
    )
    try {
      const view = render(harness(["movie-a"]))
      await act(async () => { await vi.advanceTimersByTimeAsync(80) })
      expect(pending).toHaveLength(1)

      // A later shelf mounts while the first batch is still in flight.
      view.rerender(harness(["movie-a", "movie-b"]))
      await act(async () => { await vi.advanceTimersByTimeAsync(80) })
      expect(pending).toHaveLength(2)

      await act(async () => {
        pending[0].resolve([rated("movie-a")])
        await Promise.resolve()
        await Promise.resolve()
      })
      await act(async () => { await vi.advanceTimersByTimeAsync(5000) })
      // The first response must not abort the second batch, and its cards must
      // not need a replacement request.
      expect(pending[1].signal?.aborted).toBe(false)
      expect(pending).toHaveLength(2)

      await act(async () => {
        pending[1].resolve([rated("movie-b")])
        await Promise.resolve()
        await Promise.resolve()
      })
      expect(screen.getByText("movie-a:1")).toBeTruthy()
      expect(screen.getByText("movie-b:1")).toBeTruthy()
      await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
      expect(pending).toHaveLength(2)
    } finally {
      vi.useRealTimers()
      vi.unstubAllGlobals()
    }
  })

  test("an id omitted from an available response is retried and can land late", async () => {
    vi.useFakeTimers()
    const batchCalls: string[][] = []
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith("/api/integrations/ratings")) {
        return new Response(JSON.stringify(integrationStatus), { status: 200 })
      }
      // SAFETY: the provider under test always posts a JSON `{ ids }` body.
      const { ids } = JSON.parse(String(init?.body)) as { ids: string[] }
      batchCalls.push(ids)
      // The native call that owns this title's refresh finishes between the
      // first answer and the retry, so only the retry carries the entry.
      const items = batchCalls.length < 2 ? [] : [{
        ...item,
        id: "movie-1",
        ratings: [{ sourceId: "letterboxd", rawSource: "letterboxd", value: 4.2, score: 84, votes: 10, scaleMax: 5 }],
      }]
      return new Response(JSON.stringify({
        available: true,
        effectiveOrigin: "plugin",
        retryAt: null,
        diagnostic: null,
        items,
      }), { status: 200 })
    })
    vi.stubGlobal("fetch", fetchMock)
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    client.setQueryData(queryKeys.ratingsStatus, integrationStatus)
    try {
      render(
        <QueryClientProvider client={client}>
          <RatingsProvider>
            <RatingProbe id="movie-1" />
          </RatingsProvider>
        </QueryClientProvider>,
      )
      await act(async () => { await vi.advanceTimersByTimeAsync(80) })
      expect(batchCalls).toHaveLength(1)
      expect(screen.getByText("movie-1:ready")).toBeTruthy()
      await act(async () => { await vi.advanceTimersByTimeAsync(2_100) })
      expect(batchCalls).toHaveLength(2)
      expect(screen.getByText("movie-1:1")).toBeTruthy()
      // A returned entry ends the retry cycle.
      await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
      expect(batchCalls).toHaveLength(2)
    } finally {
      vi.useRealTimers()
      vi.unstubAllGlobals()
    }
  })

  test("retries for an id a response never answers stop after the bounded attempts", async () => {
    vi.useFakeTimers()
    const batchCalls: string[][] = []
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path.endsWith("/api/integrations/ratings")) {
        return new Response(JSON.stringify(integrationStatus), { status: 200 })
      }
      // SAFETY: the provider under test always posts a JSON `{ ids }` body.
      batchCalls.push((JSON.parse(String(init?.body)) as { ids: string[] }).ids)
      return new Response(JSON.stringify({
        available: true,
        effectiveOrigin: "plugin",
        retryAt: null,
        diagnostic: null,
        items: [],
      }), { status: 200 })
    })
    vi.stubGlobal("fetch", fetchMock)
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    client.setQueryData(queryKeys.ratingsStatus, integrationStatus)
    try {
      render(
        <QueryClientProvider client={client}>
          <RatingsProvider>
            <RatingProbe id="episode-1" />
          </RatingsProvider>
        </QueryClientProvider>,
      )
      await act(async () => { await vi.advanceTimersByTimeAsync(80) })
      await act(async () => { await vi.advanceTimersByTimeAsync(60_000) })
      // One initial request plus the bounded retries, then silence.
      expect(batchCalls).toHaveLength(3)
      expect(batchCalls.every((ids) => ids.includes("episode-1"))).toBe(true)
      expect(screen.getByText("episode-1:ready")).toBeTruthy()
    } finally {
      vi.useRealTimers()
      vi.unstubAllGlobals()
    }
  })

  test("the Appearance preview renders real library cards with the unsaved overlay choices", () => {
    const movie: ItemSummary = {
      id: "preview-movie",
      kind: "Movie",
      name: "Green Horizon",
      year: 2026,
      runtimeTicks: 7_000_000_000,
      communityRating: null,
      officialRating: null,
      seriesId: null,
      seriesName: null,
      indexNumber: null,
      parentIndexNumber: null,
      primaryImageTag: "tag-movie",
      thumbImageTag: null,
      logoImageTag: null,
      backdropImageTag: null,
      childCount: null,
      premiereDate: null,
      seasonId: null,
      played: false,
      playCount: 0,
      positionTicks: 0,
      favorite: false,
    }
    const show: ItemSummary = {
      ...movie,
      id: "preview-series",
      kind: "Series",
      name: "Severance",
      year: null,
      primaryImageTag: "tag-show",
      childCount: 2,
    }
    const resumeEpisode: ItemSummary = {
      ...movie,
      id: "preview-episode",
      kind: "Episode",
      name: "What We Leave Behind",
      seriesId: show.id,
      seriesName: show.name,
      indexNumber: 1,
      parentIndexNumber: 1,
      thumbImageTag: "tag-still",
      positionTicks: 1_000_000_000,
    }
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    client.setQueryData(queryKeys.ratingsStatus, integrationStatus)
    client.setQueryData(queryKeys.status, { authenticated: true })
    client.setQueryData(queryKeys.home, {
      continueWatching: [resumeEpisode],
      rows: [{ kind: "builtIn", id: "recentlyAdded", title: "Recently added", items: [movie, show] }],
    })
    const register = vi.fn(() => vi.fn())
    const { container } = render(
      <TestProviders client={client}>
          <RatingsContext.Provider
            value={{
              items: new Map([
                [movie.id, { ...item, id: movie.id, ratings: [display("letterboxd", 4.2).rating] }],
              ]),
              selected: ["letterboxd"],
              definitions: new Map(definitions.map((definition) => [definition.id, definition])),
              register,
            }}
          >
            <Appearance />
          </RatingsContext.Provider>
      </TestProviders>,
    )
    expect(screen.getByText("Cards")).toBeTruthy()
    const cardPreviews = screen.getByRole("switch", { name: "Show pop-out previews on cards" })
    expect(cardPreviews.getAttribute("aria-checked")).toBe("true")
    const mediaInfo = screen.getByRole("switch", { name: "Show media info on cards" })
    expect(mediaInfo.getAttribute("aria-checked")).toBe("true")

    const preview = requireElement(
      container.querySelector<HTMLElement>(".appearance-preview"),
      "appearance preview",
    )
    expect(preview.dataset.mediaInfo).toBe("true")
    expect(preview.dataset.cardPreviews).toBe("true")
    // The shelf is the app's own cards over real home-feed rows: a resuming
    // episode and both a movie and a series from Recently added.
    expect(preview.querySelectorAll(".signal-card").length).toBeGreaterThanOrEqual(3)
    expect(preview.querySelector("[aria-label='Open details for Green Horizon']")).toBeTruthy()
    expect(preview.querySelector("[aria-label='Open details for Severance']")).toBeTruthy()
    expect(preview.querySelector(".card-rating-readout")).toBeTruthy()
    expect(preview.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()

    // The draft selection drives the overlays before anything is saved.
    fireEvent.click(screen.getByRole("checkbox", { name: "Letterboxd" }))
    expect(preview.querySelector("[data-rating-source-icon='letterboxd']")).toBeNull()
    fireEvent.click(screen.getByRole("checkbox", { name: "Letterboxd" }))
    expect(preview.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()

    // With previews off, the quick actions move onto the card itself.
    fireEvent.click(cardPreviews)
    expect(preview.dataset.cardPreviews).toBe("false")
    expect(preview.querySelector(".card-inline-actions")).toBeTruthy()
    fireEvent.click(mediaInfo)
    expect(preview.dataset.mediaInfo).toBe("false")
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()
    expect(screen.getAllByRole("button", { name: "Save" }).some((button) => !button.hasAttribute("disabled"))).toBe(true)
  })

  test("saved media-info visibility reaches library cards through the root appearance state", () => {
    const disabled = {
      ...clientSettings,
      appearance: { ...clientSettings.appearance, showMediaInfo: false },
    }
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, disabled)
    render(<QueryClientProvider client={client}><AppearanceSync /></QueryClientProvider>)
    expect(document.documentElement.dataset.mediaInfo).toBe("false")
    delete document.documentElement.dataset.mediaInfo
  })

  test("saved card-preview visibility reaches browsing cards through the root appearance state", () => {
    const disabled = {
      ...clientSettings,
      appearance: { ...clientSettings.appearance, cardPreviews: false },
    }
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, disabled)
    render(<QueryClientProvider client={client}><AppearanceSync /></QueryClientProvider>)
    expect(document.documentElement.dataset.cardPreviews).toBe("false")
    delete document.documentElement.dataset.cardPreviews
  })

  test("reports Companion services and the current Seerr user mapping", () => {
    const client = testQueryClient()
    client.setQueryData(queryKeys.status, { authenticated: true })
    client.setQueryData(queryKeys.companion, {
      available: true,
      compatible: true,
      checked: true,
      error: null,
      supportedApi: { min: 1, max: 1 },
      info: {
        pluginVersion: "0.2.0",
        apiVersion: 1,
        capabilities: [
          "seerr",
          "ratings-v1",
          "collection-experience-v1",
          "franchise-memberships-v1",
          "seerr-person-discovery",
          "seerr-discovery-v4",
          "seerr-request-profiles",
        ],
        services: { seerr: true, sonarr: true, radarr: false, mdblist: true, tmdb: true },
      },
    })
    client.setQueryData(queryKeys.seerrStatus, {
      linked: true,
      mapped: true,
      instance: { movie4kEnabled: false, series4kEnabled: false, partialRequestsEnabled: true },
      user: { id: 1, name: "Neo", avatar: null, jellyfinUserId: "neo" },
      capabilities: null,
      quota: null,
    })

    render(
      <TestProviders client={client} initialEntries={["/settings/integrations/companion"]}>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
      </TestProviders>,
    )

    expect(screen.getByRole("heading", { name: "MediaFlick Companion" })).toBeTruthy()
    expect(screen.getByRole("link", { name: "MediaFlick Companion" }).getAttribute("href")).toBe("/settings/integrations/companion")
    expect(screen.getByText(/This account is mapped as Neo/)).toBeTruthy()
    expect(screen.getByText("Desktop features").closest(".settings-row")?.textContent).toContain("compatible")
    expect(screen.getByText("Radarr").closest(".settings-row")?.textContent).toContain("unavailable")
    expect(screen.getByText("TMDB").closest(".settings-row")?.textContent).toContain("available")
  })

  test("reports Companion feature mismatches without treating the plugin as disconnected", () => {
    const client = testQueryClient()
    client.setQueryData(queryKeys.status, { authenticated: true })
    client.setQueryData(queryKeys.companion, {
      available: true,
      compatible: true,
      checked: true,
      error: null,
      supportedApi: { min: 1, max: 1 },
      info: {
        pluginVersion: "0.2.0",
        apiVersion: 1,
        capabilities: ["collection-experience-v1"],
        services: { seerr: false, sonarr: true, radarr: true, mdblist: true, tmdb: true },
      },
    })

    render(
      <TestProviders client={client} initialEntries={["/settings/integrations/companion"]}>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
      </TestProviders>,
    )

    expect(screen.getByText("Connection").closest(".settings-row")?.textContent).toContain("available")
    expect(screen.getByText("Movie franchises").closest(".settings-row")?.textContent).toContain("missing")
    expect(screen.getByText(/cannot supply the franchise membership data/)).toBeTruthy()
    expect(screen.getByText("Sonarr").closest(".settings-row")?.textContent).toContain("available")
  })

  test("disables all source choices without a credential/capability", () => {
    const onChange = vi.fn()
    const { rerender } = render(
      <RatingSourceSelector
        sources={definitions}
        selected={["letterboxd"]}
        enabled={false}
        onChange={onChange}
      />,
    )
    expect(screen.getByLabelText("Letterboxd").matches(":disabled")).toBe(true)

    rerender(
      <RatingSourceSelector
        sources={definitions}
        selected={["letterboxd"]}
        enabled
        onChange={onChange}
      />,
    )
    fireEvent.click(screen.getByLabelText("Rotten Tomatoes Audience"))
    expect(onChange).toHaveBeenCalledWith(["letterboxd", "popcorn"])
  })
})
