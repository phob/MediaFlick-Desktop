import { describe, expect, test } from "vitest"
import { startupScreenReady } from "@/lib/startup"

describe("home startup cover", () => {
  test("stays up until both SQLite-backed home queries settle", () => {
    const readiness = {
      statusPending: false,
      settingsPending: false,
      waitingForLibrary: false,
      showingSettings: false,
      initialHomeEnabled: true,
      homePending: true,
      billboardPending: true,
    }

    expect(startupScreenReady(readiness)).toBe(false)
    expect(startupScreenReady({ ...readiness, homePending: false })).toBe(false)
    expect(startupScreenReady({ ...readiness, homePending: false, billboardPending: false })).toBe(true)
  })
})
