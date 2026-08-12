import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test, vi } from "vitest"
import { EpisodeList } from "../src/components/detail/EpisodeList"
import { DetailRatingReadout, RatingOverlayView } from "../src/components/RatingOverlay"
import { RatingSourceIcon } from "../src/components/RatingSourceIcon"
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
import {
  Appearance,
  AppearanceSync,
  RatingsIntegration,
  RatingSourceSelector,
  RatingsSetupDialog,
  SecureCredentialField,
} from "../src/routes/Settings"
import { queryKeys } from "../src/lib/query-client"

const definitions: RatingSourceDefinition[] = [
  { id: "imdb", label: "IMDb", shortLabel: "IMDb", scaleMax: 10, format: "decimal", known: true },
  { id: "letterboxd", label: "Letterboxd", shortLabel: "LB", scaleMax: 5, format: "stars", known: true },
  { id: "tomatoes", label: "Rotten Tomatoes Critics", shortLabel: "RT", scaleMax: 100, format: "percent", known: true },
  { id: "popcorn", label: "Rotten Tomatoes Audience", shortLabel: "RT A", scaleMax: 100, format: "percent", known: true },
  { id: "metacriticuser", label: "Metacritic Users", shortLabel: "MC U", scaleMax: 10, format: "decimal", known: true },
]

function display(sourceId: string, value: number): DisplayRating {
  const definition = definitions.find((candidate) => candidate.id === sourceId)!
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
  auth: { currentMode: "api_key", supportedModes: ["api_key"], futureModes: ["public_pkce"] },
  credentialPrecedence: ["local", "plugin", "none"],
  effectiveOrigin: "local_mdblist",
  available: true,
  selectionEnabled: true,
  local: {
    mdblist: { configured: true, valid: true, validation: "valid", detail: "Valid", quota: { limit: 1000, remaining: 900, resetAt: null }, retryAt: null, lastCheckedAt: 1, storage: "os_credential_vault", usedForRatings: true },
    tmdb: { configured: false, valid: false, validation: "absent", detail: null, quota: { limit: null, remaining: null, resetAt: null }, retryAt: null, lastCheckedAt: null, storage: "os_credential_vault", usedForRatings: false },
  },
  plugin: { available: false, capability: "ratings-v1", boundaryVersion: 1, detail: "Not available" },
  sources: definitions,
  selectedSources: ["letterboxd"],
}

const clientSettings: ClientSettings = {
  client: {
    player: { playerBackend: "mpv", mpvPath: null, mpchcPath: null, defaultFullscreen: "fullscreen", markWatchedNext: "w", playerConfigured: false },
    playback: { streamingQuality: "original", skipIntro: "prompt", skipCredits: "prompt", skipRecap: "prompt", skipCommercial: "prompt" },
    application: { closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" },
  },
  appearance: { theme: "system", accent: "signal", density: "comfortable", artworkIntensity: 100, backdropIntensity: 100, reducedMotion: false, showMediaInfo: true, ratingSources: ["letterboxd"] },
  capabilities: { platform: "windows", mpchc: true, mpvInstaller: true },
  streamingQuality: "original",
  playerBackend: "mpv",
  playerConfigured: false,
  serverUrl: null,
}

function withSettings(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })
  client.setQueryData(queryKeys.settings, clientSettings)
  client.setQueryData(queryKeys.ratingsStatus, integrationStatus)
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>)
}

const item: ItemRatings = {
  id: "movie-1",
  ratings: [],
  origin: "local_mdblist",
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
  test("provides an icon for every native source and a safe future-source fallback", () => {
    const sourceIds = [
      "mdblist_score_average",
      "imdb",
      "trakt",
      "tmdb",
      "letterboxd",
      "tomatoes",
      "popcorn",
      "metacritic",
      "metacriticuser",
      "rogerebert",
      "myanimelist",
      "future-source",
    ]
    const { container } = render(<>{sourceIds.map((sourceId) => (
      <RatingSourceIcon key={sourceId} sourceId={sourceId} />
    ))}</>)

    for (const sourceId of sourceIds) {
      expect(container.querySelector(`[data-rating-source-icon="${sourceId}"]`)).toBeTruthy()
    }
  })

  test("formats each native source scale without conflating RT critics and audience", () => {
    expect(display("letterboxd", 4.25).formatted).toBe("★4.3")
    expect(display("imdb", 8.1).accessibleValue).toBe("8.1 out of 10")
    expect(display("tomatoes", 97).formatted).toBe("97%")
    expect(display("popcorn", 91).formatted).toBe("91%")
  })

  test("renders multiple available ratings as one accessible top-left definition list", () => {
    render(
      <RatingOverlayView
        itemName="The Matrix"
        ratingItem={item}
        ratings={[display("letterboxd", 4.2), display("tomatoes", 88), display("popcorn", 94)]}
      />,
    )

    const overlay = screen.getByLabelText("Ratings for The Matrix")
    expect(overlay.tagName).toBe("DL")
    expect(overlay.className).toContain("card-rating-readout")
    expect(overlay.querySelector(".card-rating-chip")).toBeNull()
    expect(overlay.querySelectorAll("[data-rating-source-icon]")).toHaveLength(3)
    expect(overlay.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()
    expect(overlay.querySelector("[data-rating-source-icon='tomatoes']")).toBeTruthy()
    expect(overlay.querySelector("[data-rating-source-icon='popcorn']")).toBeTruthy()
    expect(overlay.textContent).not.toContain("LB")
    expect(overlay.textContent).not.toContain("RT A")
    expect(overlay.querySelector(".card-rating-separator")).toBeNull()
    expect(overlay.querySelectorAll(".rating-readout-value")).toHaveLength(3)
    expect(overlay.getAttribute("data-rating-origin")).toBe("local_mdblist")
    expect(screen.getByLabelText("Letterboxd rating 4.2 out of 5")).toBeTruthy()
    expect(screen.getByLabelText("Rotten Tomatoes Critics rating 88 percent")).toBeTruthy()
    expect(screen.getByLabelText("Rotten Tomatoes Audience rating 94 percent")).toBeTruthy()
    expect(overlay.querySelector("[title*='via local MDBList']")).toBeTruthy()
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
    expect(readout.className).toContain("detail-rating-readout")
    expect(screen.getByLabelText("Letterboxd rating 4.2 out of 5")).toBeTruthy()
    expect(readout.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()
    expect(readout.textContent).not.toContain("LB")
    expect(register).toHaveBeenCalledWith(item.id)
  })

  test("shows each episode's own Jellyfin community rating", () => {
    const episode: ItemSummary = {
      id: "episode-1",
      kind: "Episode",
      name: "Good News About Hell",
      year: 2022,
      runtimeTicks: 3_000_000_000,
      communityRating: 8.4,
      officialRating: null,
      seriesId: "series-1",
      seriesName: "Severance",
      indexNumber: 1,
      parentIndexNumber: 1,
      primaryImageTag: null,
      thumbImageTag: null,
      logoImageTag: null,
      backdropImageTag: null,
      childCount: null,
      premiereDate: "2022-02-18T00:00:00Z",
      seasonId: "season-1",
      played: false,
      playCount: 0,
      positionTicks: 0,
      favorite: false,
      overview: "Mark is promoted.",
    }
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <EpisodeList episodes={[episode]} parentId="season-1" />
        </MemoryRouter>
      </QueryClientProvider>,
    )

    expect(screen.getByLabelText("Jellyfin community rating 8.4 out of 10")).toBeTruthy()
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
        effectiveOrigin: "local_mdblist",
        retryAt: null,
        diagnostic: null,
        items: [{ ...item, ratings: [{ sourceId: "letterboxd", rawSource: "letterboxd", value: 4.2, score: 84, votes: 10, scaleMax: 5 }] }],
      }), { status: 200 })
    })
    vi.stubGlobal("fetch", fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })
    client.setQueryData(queryKeys.settings, clientSettings)
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

  test("keeps saved credentials masked and reveals them only on an explicit action", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ key: "local-secret" }), { status: 200 }),
    )
    vi.stubGlobal("fetch", fetchMock)
    try {
      render(
        <SecureCredentialField
          provider="mdblist"
          label="MDBList API key"
          status={integrationStatus.local.mdblist}
          onStatus={vi.fn()}
        />,
      )
      const input = screen.getByLabelText("MDBList API key") as HTMLInputElement
      expect(input.type).toBe("password")
      expect(input.value).toBe("")
      expect(input.placeholder).toContain("••••")
      expect(screen.getByRole("button", { name: "Copy MDBList API key" })).toBeTruthy()
      expect(screen.getByRole("button", { name: "Remove MDBList API key" })).toBeTruthy()
      fireEvent.click(screen.getByRole("button", { name: "Reveal MDBList API key" }))
      expect(await screen.findByDisplayValue("local-secret")).toBeTruthy()
      expect(input.type).toBe("text")
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/integrations/ratings/credential/mdblist/reveal",
        expect.objectContaining({ method: "POST" }),
      )
    } finally {
      vi.unstubAllGlobals()
    }
  })

  test("exposes source selection in both setup Appearance and normal Appearance", () => {
    const onClose = vi.fn()
    withSettings(<RatingsSetupDialog onClose={onClose} />)
    fireEvent.click(screen.getByRole("button", { name: "Next: Appearance" }))
    expect(screen.getByRole("dialog", { name: /Ratings setup · Appearance/ })).toBeTruthy()
    expect(screen.getByLabelText("Letterboxd")).toBeTruthy()
  })

  test("normal Appearance previews and saves card overlay preferences together", () => {
    const { container } = withSettings(<Appearance />)
    expect(screen.getByText("Card overlays")).toBeTruthy()
    const mediaInfo = screen.getByRole("switch", { name: "Show media info on cards" })
    expect(mediaInfo.getAttribute("aria-checked")).toBe("true")
    expect((screen.getByLabelText("Letterboxd") as HTMLInputElement).checked).toBe(true)
    expect(screen.getByLabelText("Rotten Tomatoes Critics")).toBeTruthy()
    expect(screen.getByLabelText("Rotten Tomatoes Audience")).toBeTruthy()

    const preview = container.querySelector(".appearance-preview") as HTMLElement
    expect(preview.dataset.showMediaInfo).toBe("true")
    expect(preview.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()
    fireEvent.click(mediaInfo)
    expect(preview.dataset.showMediaInfo).toBe("false")
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()
    expect(screen.getAllByRole("button", { name: "Save" }).some((button) => !button.hasAttribute("disabled"))).toBe(true)
  })

  test("the ratings page drafts preference changes until its Save action", () => {
    withSettings(<RatingsIntegration />)
    fireEvent.click(screen.getByLabelText("Rotten Tomatoes Audience"))
    expect((screen.getByLabelText("Rotten Tomatoes Audience") as HTMLInputElement).checked).toBe(true)
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()
    expect(screen.getAllByRole("button", { name: "Save" }).some((button) => !button.hasAttribute("disabled"))).toBe(true)
  })

  test("saved media-info visibility reaches library cards through the root appearance state", () => {
    const disabled = {
      ...clientSettings,
      appearance: { ...clientSettings.appearance, showMediaInfo: false },
    }
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })
    client.setQueryData(queryKeys.settings, disabled)
    render(<QueryClientProvider client={client}><AppearanceSync /></QueryClientProvider>)
    expect(document.documentElement.dataset.mediaInfo).toBe("false")
    delete document.documentElement.dataset.mediaInfo
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
