import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, describe, expect, it, vi } from "vitest"
import { AppShell } from "@/components/AppShell"
import type { ClientSettings, PlayerState } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"

const settings = {
  client: {
    player: {
      playerBackend: "libmpv",
      mpvPath: null,
      mpchcPath: null,
      defaultFullscreen: "windowed",
      markWatchedNext: "w",
      playerConfigured: true,
    },
    playback: {
      streamingQuality: "auto",
      skipIntro: "disabled",
      skipCredits: "disabled",
      skipRecap: "disabled",
      skipCommercial: "disabled",
    },
    application: {
      closeBehavior: "exit_app",
      showScrollbars: false,
      logLevel: "info",
    },
  },
  appearance: {
    theme: "dark",
    accent: "signal",
    density: "comfortable",
    artworkIntensity: 1,
    backdropIntensity: 1,
    reducedMotion: false,
    cardPreviews: true,
    showMediaInfo: true,
    ratingSources: [],
  },
  capabilities: {
    platform: "windows",
    libmpv: true,
    integratedLibmpvOverlay: true,
    mpchc: true,
    mpvInstaller: true,
  },
  streamingQuality: "auto",
  playerBackend: "libmpv",
  playerConfigured: true,
  serverUrl: "http://localhost:8096",
} satisfies ClientSettings

describe("integrated libmpv overlay", () => {
  afterEach(() => {
    document.documentElement.removeAttribute("data-libmpv-playback")
    document.documentElement.removeAttribute("data-libmpv-cursor-hidden")
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it("replaces library chrome and hides player controls while video keeps playing", () => {
    vi.useFakeTimers()
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({
        matches: false,
        media: "",
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      })),
    )
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    })
    client.setQueryData(queryKeys.settings, settings)
    client.setQueryData<PlayerState>(queryKeys.playerState, {
      active: true,
      playbackId: "1",
      positionMs: 12_000,
      durationMs: 120_000,
      paused: false,
    })

    const view = render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AppShell>
            <div>Library chrome</div>
          </AppShell>
        </MemoryRouter>
      </QueryClientProvider>,
    )

    expect(screen.getByLabelText("MediaFlick")).not.toBeNull()
    expect(screen.getByRole("button", { name: "Stop" })).not.toBeNull()
    expect(screen.queryByText("Library chrome")).toBeNull()
    expect(document.documentElement.hasAttribute("data-libmpv-playback")).toBe(true)

    const brand = view.container.querySelector<HTMLElement>(".libmpv-overlay-brand")
    const playerBar = view.container.querySelector<HTMLElement>(".player-bar")?.parentElement
    expect(brand?.dataset.visible).toBe("true")
    expect(playerBar?.dataset.visible).toBe("true")

    fireEvent.mouseMove(window, { clientX: 500, clientY: 300 })
    act(() => vi.advanceTimersByTime(3000))
    expect(brand?.dataset.visible).toBe("false")
    expect(playerBar?.dataset.visible).toBe("false")
    expect(document.documentElement.hasAttribute("data-libmpv-cursor-hidden")).toBe(true)

    fireEvent.mouseMove(window, { clientX: 500, clientY: 300 })
    fireEvent.mouseMove(window, { clientX: 509, clientY: 300 })
    expect(playerBar?.dataset.visible).toBe("false")
    expect(document.documentElement.hasAttribute("data-libmpv-cursor-hidden")).toBe(true)

    fireEvent.mouseMove(window, { clientX: 510, clientY: 300 })
    expect(playerBar?.dataset.visible).toBe("false")
    expect(document.documentElement.hasAttribute("data-libmpv-cursor-hidden")).toBe(false)

    fireEvent.mouseMove(window, {
      clientX: window.innerWidth / 2,
      clientY: window.innerHeight - 1,
    })
    expect(playerBar?.dataset.visible).toBe("true")

    act(() => {
      document.dispatchEvent(new Event("fullscreenchange"))
      vi.advanceTimersByTime(2000)
      window.dispatchEvent(new Event("resize"))
      vi.advanceTimersByTime(2999)
    })
    expect(playerBar?.dataset.visible).toBe("true")

    act(() => vi.advanceTimersByTime(1))
    expect(playerBar?.dataset.visible).toBe("false")

    act(() => {
      client.setQueryData<PlayerState>(queryKeys.playerState, (current) => ({
        ...current!,
        paused: true,
      }))
      vi.advanceTimersByTime(3000)
    })
    expect(playerBar?.dataset.visible).toBe("true")
    expect(document.documentElement.hasAttribute("data-libmpv-cursor-hidden")).toBe(false)

    view.unmount()
    expect(document.documentElement.hasAttribute("data-libmpv-playback")).toBe(false)
    expect(document.documentElement.hasAttribute("data-libmpv-cursor-hidden")).toBe(false)
  })
})
