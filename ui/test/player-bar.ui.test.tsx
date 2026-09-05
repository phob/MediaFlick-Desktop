import { QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { PlayerBar } from "@/components/PlayerBar"
import type { PlayerState } from "@/lib/api"
import { DEFAULT_COMFORT } from "@/lib/viewing"
import { queryKeys } from "@/lib/query-client"
import { testQueryClient } from "./test-query-client"

afterEach(() => vi.unstubAllGlobals())

describe("player bar controls", () => {
  it("switches the live audio and subtitle tracks reported by mpv", async () => {
    const commands: unknown[] = []
    const playerState: PlayerState = {
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
    }
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
    client.setQueryData(queryKeys.settings, {client: {player: {playerBackend: "libmpv"}, playback: {comfort: DEFAULT_COMFORT}}})
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

  it("sends seek, mute, and fullscreen commands without exposing playback speed", async () => {
    const commands: unknown[] = []
    const playerState: PlayerState = {
      active: true,
      positionMs: 12_000,
      durationMs: 120_000,
      paused: false,
      volume: 80,
      mute: false,
      playMethod: "DirectPlay",
      diagnostics: {
        bufferedUntilMs: 60_000,
        buffering: false,
        droppedFrames: 2,
        frameRate: 23.976,
      },
      capabilities: {
        chapterMarkers: true,
        externalSubtitles: true,
        injectedHotkeys: true,
        absoluteVolume: true,
        pushesPosition: true,
        fullscreen: true,
        playbackTuning: true,
      },
    }
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
    client.setQueryData(queryKeys.settings, {client: {player: {playerBackend: "libmpv"}, playback: {comfort: DEFAULT_COMFORT}}})
    client.setQueryData<PlayerState>(queryKeys.playerState, playerState)

    const view = render(
      <QueryClientProvider client={client}>
        <PlayerBar />
      </QueryClientProvider>,
    )

    fireEvent.click(view.getByRole("button", { name: "Back 10 seconds" }))
    fireEvent.click(view.getByRole("button", { name: "Mute" }))
    fireEvent.click(view.getByRole("button", { name: "Toggle fullscreen" }))

    await waitFor(() => {
      expect(commands).toContainEqual({ command: "seek", positionMs: 2_000 })
      expect(commands).toContainEqual({ command: "set-mute", mute: true })
      expect(commands).toContainEqual({ command: "toggle-fullscreen" })
    })
    fireEvent.pointerDown(view.getByRole("button", { name: "Playback settings" }), {
      button: 0,
      ctrlKey: false,
      pointerType: "mouse",
    })
    expect(await screen.findByText("Direct play")).toBeTruthy()
    expect(await screen.findByText("23.98 fps · 2 dropped frames")).toBeTruthy()
    expect(view.queryByText(/playback speed/i)).toBeNull()
  })
})


it("uses saved built-in seek intervals and letter shortcuts", async () => {
  const commands: unknown[] = []
  const player: PlayerState = {active:true, positionMs:12000, durationMs:120000, paused:false}
  vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
    if (init?.body) commands.push(JSON.parse(String(init.body)))
    return new Response(JSON.stringify(init?.body ? {accepted:true} : player), {status:200})
  }))
  const client = testQueryClient()
  client.setQueryData(queryKeys.playerState, player)
  client.setQueryData(queryKeys.settings, {client:{player:{playerBackend:"libmpv"}, playback:{comfort:{...DEFAULT_COMFORT, seekBackSeconds:5, seekForwardSeconds:7, pauseKey:"p"}}}})
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)
  fireEvent.keyDown(window, {key:"ArrowRight"})
  fireEvent.keyDown(window, {key:"p"})
  await waitFor(() => {
    expect(commands).toContainEqual({command:"seek", positionMs:19000})
    expect(commands).toContainEqual({command:"pause"})
  })
  expect(screen.getByRole("button", {name:"Forward 7 seconds"})).toBeTruthy()
})
