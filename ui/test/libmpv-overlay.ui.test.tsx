import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { AppShell } from "@/components/AppShell"
import type { PlayerState } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"
import { playerSnapshot } from "./support/fixtures"
import { clientSettingsFixture } from "./support/settings"

// Longer than any plausible auto-hide delay, so the test does not pin one.
const IDLE_MS = 10_000

describe("integrated libmpv overlay", () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it("replaces library chrome with auto-hiding controls that stay while paused", () => {
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
    const settings = clientSettingsFixture()
    const client = testQueryClient()
    client.setQueryData(queryKeys.settings, {
      ...settings,
      capabilities: { ...settings.capabilities, integratedLibmpvOverlay: true },
    })
    client.setQueryData<PlayerState>(queryKeys.playerState, playerSnapshot({
      active: true,
      playbackId: 1,
      positionMs: 12_000,
      durationMs: 120_000,
      paused: false,
    }))

    render(
      <TestProviders client={client}>
        <AppShell>
          <div>Library chrome</div>
        </AppShell>
      </TestProviders>,
    )

    expect(screen.queryByText("Library chrome")).toBeNull()
    expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy()

    fireEvent.mouseMove(window, { clientX: 500, clientY: 300 })
    act(() => vi.advanceTimersByTime(IDLE_MS))
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull()

    fireEvent.mouseMove(window, { clientX: window.innerWidth / 2, clientY: window.innerHeight - 1 })
    expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy()

    act(() => {
      client.setQueryData<PlayerState>(queryKeys.playerState, (current) =>
        current ? { ...current, paused: true } : current,
      )
      vi.advanceTimersByTime(IDLE_MS)
    })
    expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy()
  })
})
