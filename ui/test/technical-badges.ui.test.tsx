import { DEFAULT_COMFORT } from "@/lib/viewing"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings } from "../src/lib/api"
import { jsonStringArray } from "../src/lib/json"
import { queryKeys } from "../src/lib/query-client"
import { TechnicalProvider } from "../src/lib/technical"
import { useCardTechnical } from "../src/lib/technical-context"
import { parseJsonObject } from "./support/fixtures"
import { testQueryClient } from "./test-query-client"

const clientSettings: ClientSettings = {
  client: {
    player: { playerBackend: "mpv", mpvPath: null, mpchcPath: null, defaultFullscreen: "fullscreen", markWatchedNext: "w", playerConfigured: false },
    playback: { comfort: DEFAULT_COMFORT, streamingQuality: "original", skipIntro: "prompt", skipCredits: "prompt", skipRecap: "prompt", skipCommercial: "prompt" },
    application: { closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" },
  },
  appearance: { theme: "system", accent: "signal", density: "comfortable", artworkIntensity: 100, backdropIntensity: 100, reducedMotion: false, cardPreviews: true, showMediaInfo: true, ratingSources: [] },
  capabilities: { platform: "windows", libmpv: true, mpchc: true, mpvInstaller: true },
  serverUrl: null,
}

function TechnicalProbe({ id, visible = true }: { id: string; visible?: boolean }) {
  const streams = useCardTechnical(id, visible)
  return <span>{streams ? `${id}:${streams[0]?.codec}` : `${id}:pending`}</span>
}

function requestIds(init?: RequestInit) {
  const ids = jsonStringArray(parseJsonObject(String(init?.body)).ids)
  if (!ids) throw new Error("Expected a technical batch id list")
  return ids
}

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe("live card technical channel", () => {
  test("movie and series cards coalesce into one live batch keyed by id", async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      expect(String(input)).toBe("/api/technical/batch")
      return new Response(
        JSON.stringify({
          items: [
            {
              id: "movie-1",
              mediaStreams: [{ index: 0, type: "Video", codec: "hevc" }],
            },
            {
              id: "series-1",
              mediaStreams: [{ index: 0, type: "Video", codec: "h264" }],
            },
          ],
        }),
        { status: 200 },
      )
    })
    vi.stubGlobal("fetch", fetchMock)

    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    render(
      <QueryClientProvider client={client}>
        <TechnicalProvider>
          <TechnicalProbe id="movie-1" />
          <TechnicalProbe id="movie-1" />
          <TechnicalProbe id="series-1" />
        </TechnicalProvider>
      </QueryClientProvider>,
    )

    // Cards commit before the coalescing timer fires a single request.
    expect(screen.getAllByText("movie-1:pending")).toHaveLength(2)
    expect(fetchMock).not.toHaveBeenCalled()
    await act(async () => {
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(JSON.parse(String(fetchMock.mock.calls[0][1]?.body))).toEqual({
      ids: ["movie-1", "series-1"],
    })
    expect(screen.getAllByText("movie-1:hevc")).toHaveLength(2)
    expect(screen.getByText("series-1:h264")).toBeTruthy()
  })

  test("only visible cards enter a technical batch", async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const ids = requestIds(init)
      return new Response(JSON.stringify({ items: ids.map((id) => ({ id, mediaStreams: [] })) }), {
        status: 200,
      })
    })
    vi.stubGlobal("fetch", fetchMock)

    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    const view = (secondVisible: boolean) => (
      <QueryClientProvider client={client}>
        <TechnicalProvider>
          <TechnicalProbe id="visible" />
          <TechnicalProbe id="offscreen" visible={secondVisible} />
        </TechnicalProvider>
      </QueryClientProvider>
    )
    const { rerender } = render(view(false))

    await act(async () => {
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(JSON.parse(String(fetchMock.mock.calls[0][1]?.body))).toEqual({ ids: ["visible"] })

    rerender(view(true))
    await act(async () => {
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(JSON.parse(String(fetchMock.mock.calls[1][1]?.body))).toEqual({ ids: ["offscreen"] })
  })

  test("a library or parent-context change drops cached streams even for invisible cards", async () => {
    vi.useFakeTimers()
    let codec = "h264"
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const ids = requestIds(init)
      expect(String(input)).toBe("/api/technical/batch")
      return new Response(
        JSON.stringify({
          items: ids.map((id) => ({
            id,
            mediaStreams: [{ index: 0, type: "Video", codec }],
          })),
        }),
        { status: 200 },
      )
    })
    vi.stubGlobal("fetch", fetchMock)

    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    const view = (ids: string[]) => (
      <QueryClientProvider client={client}>
        <TechnicalProvider>
          {ids.map((id) => <TechnicalProbe key={id} id={id} />)}
        </TechnicalProvider>
      </QueryClientProvider>
    )
    const { rerender } = render(view(["movie-1", "movie-2"]))
    await act(async () => {
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(screen.getByText("movie-1:h264")).toBeTruthy()

    // movie-2 scrolls away, then an episode change invalidates the mounted
    // series through its parent context as well as the direct movie id.
    rerender(view(["movie-1"]))
    codec = "av1"
    await act(async () => {
      window.dispatchEvent(
        new CustomEvent("mediaflick-desktop-shell", {
          detail: {
            type: "library-changed",
            payload: { itemIds: ["movie-2"], contextIds: ["movie-1"] },
          },
        }),
      )
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })

    // Only the mounted card refetches now…
    expect(JSON.parse(String(fetchMock.mock.calls[1][1]?.body))).toEqual({ ids: ["movie-1"] })
    expect(screen.getByText("movie-1:av1")).toBeTruthy()

    // …but the unmounted card was invalidated too, so remounting it fetches
    // fresh streams instead of reusing the pre-change cache entry.
    rerender(view(["movie-1", "movie-2"]))
    expect(screen.getByText("movie-2:pending")).toBeTruthy()
    await act(async () => {
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(JSON.parse(String(fetchMock.mock.calls[2][1]?.body))).toEqual({ ids: ["movie-2"] })
    expect(screen.getByText("movie-2:av1")).toBeTruthy()
  })

  test("live streams are refreshed after their bounded stale interval", async () => {
    vi.useFakeTimers()
    let codec = "h264"
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const ids = requestIds(init)
      return new Response(
        JSON.stringify({
          items: ids.map((id) => ({
            id,
            mediaStreams: [{ index: 0, type: "Video", codec }],
          })),
        }),
        { status: 200 },
      )
    })
    vi.stubGlobal("fetch", fetchMock)

    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, clientSettings)
    render(
      <QueryClientProvider client={client}>
        <TechnicalProvider>
          <TechnicalProbe id="movie-1" />
        </TechnicalProvider>
      </QueryClientProvider>,
    )
    await act(async () => {
      vi.advanceTimersByTime(80)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(screen.getByText("movie-1:h264")).toBeTruthy()

    codec = "av1"
    await act(async () => {
      // The one-minute scan bounds a fifteen-minute TTL to at most sixteen.
      vi.advanceTimersByTime(16 * 60_000 + 80)
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(screen.getByText("movie-1:av1")).toBeTruthy()
  })

  test("hidden media info fetches nothing", async () => {
    vi.useFakeTimers()
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }))
    vi.stubGlobal("fetch", fetchMock)

    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, {
      ...clientSettings,
      appearance: { ...clientSettings.appearance, showMediaInfo: false },
    })
    render(
      <QueryClientProvider client={client}>
        <TechnicalProvider>
          <TechnicalProbe id="movie-1" />
        </TechnicalProvider>
      </QueryClientProvider>,
    )

    await act(async () => {
      vi.advanceTimersByTime(200)
      await Promise.resolve()
    })
    expect(fetchMock).not.toHaveBeenCalled()
  })

  test("unknown settings do not race a saved hidden-media preference", async () => {
    vi.useFakeTimers()
    let finishSettings: ((response: Response) => void) | undefined
    const settingsResponse = new Promise<Response>((resolve) => {
      finishSettings = resolve
    })
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      if (String(input) === "/api/settings") return settingsResponse
      return Promise.resolve(new Response(JSON.stringify({ items: [] }), { status: 200 }))
    })
    vi.stubGlobal("fetch", fetchMock)

    const client = testQueryClient()
    render(
      <QueryClientProvider client={client}>
        <TechnicalProvider>
          <TechnicalProbe id="movie-1" />
        </TechnicalProvider>
      </QueryClientProvider>,
    )

    await act(async () => {
      vi.advanceTimersByTime(200)
      await Promise.resolve()
    })
    expect(fetchMock.mock.calls.map(([path]) => String(path))).toEqual(["/api/settings"])

    if (!finishSettings) throw new Error("Expected the settings request to be pending")
    finishSettings(
      new Response(
        JSON.stringify({
          ...clientSettings,
          appearance: { ...clientSettings.appearance, showMediaInfo: false },
        }),
        { status: 200 },
      ),
    )
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
      vi.advanceTimersByTime(200)
    })
    expect(fetchMock.mock.calls.map(([path]) => String(path))).toEqual(["/api/settings"])
  })
})
