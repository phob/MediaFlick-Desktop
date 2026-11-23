import { QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, describe, expect, it, vi } from "vitest"
import { PlayerBar } from "@/components/PlayerBar"
import type { PlayerState } from "@/lib/api"
import { usePlaybackKeys } from "@/lib/playback-keys"
import { DEFAULT_COMFORT } from "./support/settings"
import { queryKeys } from "@/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { playerSnapshot } from "./support/fixtures"

afterEach(() => vi.unstubAllGlobals())

describe("player bar controls", () => {
  it("switches the live audio and subtitle tracks reported by mpv", async () => {
    const commands: unknown[] = []
    const playerState: PlayerState = playerSnapshot({
      active: true,
      positionMs: 12_000,
      durationMs: 120_000,
      paused: false,
      tracks: [
        {
          id: 1,
          kind: "audio",
          language: "eng",
          title: "Original",
          codec: "aac",
          selected: true,
          external: false,
        },
        {
          id: 2,
          kind: "audio",
          language: "jpn",
          title: "Commentary",
          codec: "dts",
          selected: false,
          external: false,
        },
        {
          id: 3,
          kind: "subtitle",
          language: "eng",
          title: "English SDH",
          codec: "subrip",
          selected: false,
          external: true,
        },
      ],
    })
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
        if (init?.body) commands.push(JSON.parse(String(init.body)))
        const response = init?.body ? { accepted: true } : playerState
        return new Response(JSON.stringify(response), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      }),
    )
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, {client: {player: {playerBackend: "libmpv", comfort: DEFAULT_COMFORT}}})
    client.setQueryData<PlayerState>(queryKeys.playerState, playerState)

    render(
      <QueryClientProvider client={client}>
        <PlayerBar />
      </QueryClientProvider>,
    )

    fireEvent.pointerDown(screen.getByRole("button", { name: "Audio track" }), {
      button: 0,
      ctrlKey: false,
      pointerType: "mouse",
    })
    fireEvent.click(await screen.findByRole("menuitemradio", { name: /Commentary/ }))
    await waitFor(() => {
      expect(commands).toContainEqual({ command: "set-audio-track", audioTrack: 2 })
    })

    fireEvent.pointerDown(screen.getByRole("button", { name: "Subtitles" }), {
      button: 0,
      ctrlKey: false,
      pointerType: "mouse",
    })
    fireEvent.click(await screen.findByRole("menuitemradio", { name: /English SDH/ }))

    await waitFor(() => {
      expect(commands).toContainEqual({ command: "set-subtitle-track", subtitleTrack: 3 })
    })
  })

  it("sends mute and fullscreen commands", async () => {
    const commands: unknown[] = []
    const playerState: PlayerState = playerSnapshot({
      active: true,
      positionMs: 12_000,
      durationMs: 120_000,
      paused: false,
      volume: 80,
      mute: false,
    })
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
        if (init?.body) commands.push(JSON.parse(String(init.body)))
        return new Response(JSON.stringify(init?.body ? { accepted: true } : playerState), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      }),
    )
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, {client: {player: {playerBackend: "libmpv", comfort: DEFAULT_COMFORT}}})
    client.setQueryData<PlayerState>(queryKeys.playerState, playerState)

    const view = render(
      <QueryClientProvider client={client}>
        <PlayerBar />
      </QueryClientProvider>,
    )

    fireEvent.click(view.getByRole("button", { name: "Mute" }))
    fireEvent.click(view.getByRole("button", { name: "Toggle fullscreen" }))

    await waitFor(() => {
      expect(commands).toContainEqual({ command: "set-mute", mute: true })
      expect(commands).toContainEqual({ command: "toggle-fullscreen" })
    })
  })
})


it("uses saved built-in seek intervals", async () => {
  const commands: unknown[] = []
  const player: PlayerState = playerSnapshot({active:true, positionMs:12000, durationMs:120000, paused:false})
  vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
    if (init?.body) commands.push(JSON.parse(String(init.body)))
    return new Response(JSON.stringify(init?.body ? {accepted:true} : player), {status:200})
  }))
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, player)
  client.setQueryData(queryKeys.settings, {client:{player:{playerBackend:"libmpv", comfort:{...DEFAULT_COMFORT, seekBackSeconds:5, seekForwardSeconds:7}}}})
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)
  fireEvent.keyDown(window, {key:"ArrowRight"})
  await waitFor(() => expect(commands).toContainEqual({command:"seek", positionMs:19000}))
})

it("accumulates rapid video keyboard seeks without losing presses when backend position lags", async () => {
  const commands: Array<{ command: string; positionMs?: number }> = []
  let backendState: PlayerState = playerSnapshot({ active: true, playbackId: 7, positionMs: 10_000, durationMs: 600_000, paused: false })
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.body) commands.push(JSON.parse(String(init.body)))
      return new Response(JSON.stringify(init?.body ? { accepted: true } : backendState), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    }),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  client.setQueryData(queryKeys.settings, { client: { player: { playerBackend: "libmpv", comfort: DEFAULT_COMFORT } } })
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)

  for (let i = 0; i < 6; i++) fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 190_000 }))
  expect(screen.getByRole("slider", { name: "Playback position" }).getAttribute("aria-valuenow")).toBe("190000")

  fireEvent.keyDown(window, { key: "ArrowLeft" })
  fireEvent.keyDown(window, { key: "ArrowLeft" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 170_000 }))

  backendState = playerSnapshot({ active: true, playbackId: 7, positionMs: 170_500, durationMs: 600_000, paused: false })
  act(() => { client.setQueryData(queryKeys.playerState, backendState) })
  fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 200_500 }))
})

it("clamps rapid seeks at zero and duration and reverses immediately", async () => {
  const commands: Array<{ command: string; positionMs?: number }> = []
  const backendState: PlayerState = playerSnapshot({ active: true, playbackId: 9, positionMs: 5_000, durationMs: 50_000, paused: false })
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.body) commands.push(JSON.parse(String(init.body)))
      return new Response(JSON.stringify(init?.body ? { accepted: true } : backendState), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    }),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  client.setQueryData(queryKeys.settings, { client: { player: { playerBackend: "libmpv", comfort: DEFAULT_COMFORT } } })
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)

  fireEvent.keyDown(window, { key: "ArrowLeft" })
  fireEvent.keyDown(window, { key: "ArrowLeft" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 0 }))

  fireEvent.keyDown(window, { key: "ArrowRight" })
  fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 50_000 }))

  fireEvent.keyDown(window, { key: "ArrowLeft" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 40_000 }))
})

function AudioKeysHarness() {
  usePlaybackKeys()
  return <PlayerBar />
}

it("accumulates rapid audio keyboard seeks when backend position lags", async () => {
  const commands: Array<{ command: string; positionMs?: number }> = []
  const backendState: PlayerState = playerSnapshot({
    active: true,
    playbackId: 11,
    itemId: "track-1",
    positionMs: 15_000,
    durationMs: 300_000,
    paused: false,
    nowPlaying: {
      kind: "audio",
      mediaKind: "music",
      current: {
        itemId: "track-1",
        title: "Track 1",
        artists: ["Artist"],
        album: "Album",
        albumId: "album-1",
        artwork: null,
        discNumber: 1,
        trackNumber: 1,
        runtimeTicks: 3_000_000_000,
      },
      next: null,
      playlistItemId: "pl-1",
      index: 0,
      length: 1,
      canPrevious: false,
      canNext: false,
      phase: "playing",
    },
  })
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.body) commands.push(JSON.parse(String(init.body)))
      return new Response(JSON.stringify(init?.body ? { accepted: true } : backendState), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    }),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <AudioKeysHarness />
      </QueryClientProvider>
    </MemoryRouter>,
  )

  fireEvent.keyDown(document.body, { key: "ArrowRight" })
  await waitFor(() => expect(commands).toContainEqual({ command: "seek", positionMs: 45_000 }))
  fireEvent.keyDown(document.body, { key: "ArrowRight" })
  fireEvent.keyDown(document.body, { key: "ArrowRight" })
  await waitFor(() => expect(commands.at(-1)).toEqual({ command: "seek", positionMs: 105_000 }))
  expect(screen.getByRole("slider", { name: "Playback position" }).getAttribute("aria-valuenow")).toBe("105000")
})

it("serializes and coalesces in-flight rapid seek commands in order", async () => {
  const sent: Array<{ command: string; positionMs?: number }> = []
  const releases: Array<() => void> = []
  const backendState: PlayerState = playerSnapshot({ active: true, playbackId: 12, positionMs: 10_000, durationMs: 600_000, paused: false })
  vi.stubGlobal(
    "fetch",
    vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.body) {
        sent.push(JSON.parse(String(init.body)))
        return new Promise<Response>((resolve) => {
          releases.push(() =>
            resolve(
              new Response(JSON.stringify({ accepted: true }), {
                status: 200,
                headers: { "Content-Type": "application/json" },
              }),
            ),
          )
        })
      }
      return Promise.resolve(
        new Response(JSON.stringify(backendState), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      )
    }),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  client.setQueryData(queryKeys.settings, { client: { player: { playerBackend: "libmpv", comfort: DEFAULT_COMFORT } } })
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)

  for (let i = 0; i < 6; i++) fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() => expect(sent).toEqual([{ command: "seek", positionMs: 40_000 }]))
  expect(releases).toHaveLength(1)
  expect(screen.getByRole("slider", { name: "Playback position" }).getAttribute("aria-valuenow")).toBe("190000")

  act(() => { releases[0]() })
  await waitFor(() =>
    expect(sent).toEqual([
      { command: "seek", positionMs: 40_000 },
      { command: "seek", positionMs: 190_000 },
    ]),
  )
  act(() => { releases[1]() })
})

it("does not wedge seeking when the player bar unmounts while a seek is in flight", async () => {
  const sent: Array<{ command: string; positionMs?: number }> = []
  const releases: Array<() => void> = []
  const backendState: PlayerState = playerSnapshot({ active: true, playbackId: 21, positionMs: 10_000, durationMs: 600_000, paused: false })
  vi.stubGlobal(
    "fetch",
    vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.body) {
        sent.push(JSON.parse(String(init.body)))
        return new Promise<Response>((resolve) => {
          releases.push(() =>
            resolve(
              new Response(JSON.stringify({ accepted: true }), {
                status: 200,
                headers: { "Content-Type": "application/json" },
              }),
            ),
          )
        })
      }
      return Promise.resolve(
        new Response(JSON.stringify(backendState), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      )
    }),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  client.setQueryData(queryKeys.settings, { client: { player: { playerBackend: "libmpv", comfort: DEFAULT_COMFORT } } })
  const firstMount = render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)

  fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() => expect(sent).toEqual([{ command: "seek", positionMs: 40_000 }]))

  firstMount.unmount()
  await act(async () => {
    releases[0]()
    await Promise.resolve()
  })

  const nextState = playerSnapshot({ active: true, playbackId: 22, positionMs: 20_000, durationMs: 600_000, paused: false })
  client.setQueryData(queryKeys.playerState, nextState)
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)

  fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() =>
    expect(sent).toEqual([
      { command: "seek", positionMs: 40_000 },
      { command: "seek", positionMs: 50_000 },
    ]),
  )
  act(() => { releases[1]() })
})

it("dispatches a queued seek back to the reported position and unfreezes net-zero bursts immediately", async () => {
  const sent: Array<{ command: string; positionMs?: number }> = []
  const releases: Array<() => void> = []
  let backendState: PlayerState = playerSnapshot({ active: true, playbackId: 23, positionMs: 10_000, durationMs: 600_000, paused: false })
  vi.stubGlobal(
    "fetch",
    vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.body) {
        sent.push(JSON.parse(String(init.body)))
        return new Promise<Response>((resolve) => {
          releases.push(() =>
            resolve(
              new Response(JSON.stringify({ accepted: true }), {
                status: 200,
                headers: { "Content-Type": "application/json" },
              }),
            ),
          )
        })
      }
      return Promise.resolve(
        new Response(JSON.stringify(backendState), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      )
    }),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  client.setQueryData(queryKeys.settings, {
    client: { player: { playerBackend: "libmpv", comfort: { ...DEFAULT_COMFORT, seekBackSeconds: 30, seekForwardSeconds: 30 } } },
  })
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)

  fireEvent.keyDown(window, { key: "ArrowRight" })
  await waitFor(() => expect(sent).toEqual([{ command: "seek", positionMs: 40_000 }]))
  fireEvent.keyDown(window, { key: "ArrowLeft" })

  await act(async () => {
    releases[0]()
    await Promise.resolve()
  })
  await waitFor(() =>
    expect(sent).toEqual([
      { command: "seek", positionMs: 40_000 },
      { command: "seek", positionMs: 10_000 },
    ]),
  )
  await act(async () => {
    releases[1]()
    await Promise.resolve()
  })

  backendState = playerSnapshot({ active: true, playbackId: 23, positionMs: 10_500, durationMs: 600_000, paused: false })
  act(() => { client.setQueryData(queryKeys.playerState, backendState) })
  await waitFor(() => {
    expect(screen.getByRole("slider", { name: "Playback position" }).getAttribute("aria-valuenow")).toBe("10500")
  })
})

function RenderCountingAudioKeys({ onRender }: { onRender: () => void }) {
  onRender()
  usePlaybackKeys()
  return null
}

it("does not re-render usePlaybackKeys host on seek", async () => {
  let renders = 0
  const backendState: PlayerState = playerSnapshot({
    active: true,
    playbackId: 24,
    itemId: "track-2",
    positionMs: 15_000,
    durationMs: 300_000,
    paused: false,
    nowPlaying: {
      kind: "audio",
      mediaKind: "music",
      current: {
        itemId: "track-2",
        title: "Track 2",
        artists: ["Artist"],
        album: "Album",
        albumId: "album-1",
        artwork: null,
        discNumber: 1,
        trackNumber: 2,
        runtimeTicks: 3_000_000_000,
      },
      next: null,
      playlistItemId: "pl-2",
      index: 0,
      length: 1,
      canPrevious: false,
      canNext: false,
      phase: "playing",
    },
  })
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) =>
      new Response(JSON.stringify(init?.body ? { accepted: true } : backendState), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    ),
  )
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, backendState)
  render(
    <QueryClientProvider client={client}>
      <RenderCountingAudioKeys onRender={() => { renders += 1 }} />
    </QueryClientProvider>,
  )
  expect(renders).toBe(1)
  fireEvent.keyDown(document.body, { key: "ArrowRight" })
  fireEvent.keyDown(document.body, { key: "ArrowRight" })
  fireEvent.keyDown(document.body, { key: "ArrowRight" })
  await waitFor(() => expect(renders).toBe(1))
})
