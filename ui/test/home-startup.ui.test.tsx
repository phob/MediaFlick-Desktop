import { render, screen } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
import { LoadingScreen } from "@/components/LoadingScreen"
import { api } from "@/lib/api"
import { resetWindowRevealed, shouldRevealLoadingScreen, startupScreenReady } from "@/lib/startup"

afterEach(() => {
  vi.restoreAllMocks()
  resetWindowRevealed()
})

describe("home startup cover", () => {
  test("stays up until both SQLite-backed home queries settle", () => {
    const readiness = {
      statusPending: false,
      settingsPending: false,
      catalogLoading: false,
      showingSettings: false,
      initialHomeEnabled: true,
      homePending: true,
      billboardPending: true,
    }

    expect(startupScreenReady(readiness)).toBe(false)
    expect(startupScreenReady({ ...readiness, homePending: false })).toBe(false)
    expect(startupScreenReady({ ...readiness, homePending: false, billboardPending: false })).toBe(true)
  })

  test("reveals the window immediately when restarting with an unready catalog", () => {
    const readySpy = vi.spyOn(api.shell, "windowReady").mockResolvedValue({ queued: true })
    const unreadyCatalog = {
      statusPending: false,
      settingsPending: false,
      catalogLoading: true,
      showingSettings: false,
      initialHomeEnabled: false,
      homePending: true,
      billboardPending: true,
    }

    expect(startupScreenReady(unreadyCatalog)).toBe(false)
    expect(shouldRevealLoadingScreen(unreadyCatalog)).toBe(true)
    expect(shouldRevealLoadingScreen({ ...unreadyCatalog, statusPending: true })).toBe(false)
    expect(shouldRevealLoadingScreen({ ...unreadyCatalog, catalogLoading: false })).toBe(false)

    render(<LoadingScreen ready={false} revealWhileLoading />)
    expect(screen.getByRole("status", { name: "Loading MediaFlick" })).toBeTruthy()
    expect(readySpy).toHaveBeenCalledTimes(1)
  })
})
