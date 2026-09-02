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
    expect(startupScreenReady({ ...readiness, settingsPending: true })).toBe(false)
  })

  test("keeps library startup gated while settings remains directly available", () => {
    const readiness = {
      statusPending: false,
      settingsPending: false,
      waitingForLibrary: true,
      showingSettings: false,
      initialHomeEnabled: false,
      homePending: false,
      billboardPending: false,
    }

    expect(startupScreenReady(readiness)).toBe(false)
    expect(startupScreenReady({ ...readiness, showingSettings: true })).toBe(true)
  })
})
