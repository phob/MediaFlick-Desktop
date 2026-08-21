import { describe, expect, test } from "vitest"
import { windowTitle } from "../src/lib/window-title"

describe("window title", () => {
  test("named sections label themselves", () => {
    expect(windowTitle("/settings/client/player", { authenticated: true })).toBe("Settings — MediaFlick")
    expect(windowTitle("/library?kind=Movie", { authenticated: true })).toBe("Library — MediaFlick")
    expect(windowTitle("/calendar", { authenticated: true })).toBe("Releases — MediaFlick")
    expect(windowTitle("/discover/movies/603", { authenticated: true })).toBe("Discover — MediaFlick")
    expect(windowTitle("/requests", { authenticated: true })).toBe("Requests — MediaFlick")
  })

  test("home separates a signed-out gate from the signed-in surface", () => {
    expect(windowTitle("/", { authenticated: false })).toBe("Sign in — MediaFlick")
    expect(windowTitle("/", { authenticated: true })).toBe("Home — MediaFlick")
  })

  test("item details lead with the title once it is known", () => {
    const id = "771"
    expect(windowTitle(`/item/${id}`, { authenticated: true })).toBe("MediaFlick")
    expect(windowTitle(`/item/${id}`, { authenticated: true, itemTitle: null })).toBe("MediaFlick")
    expect(windowTitle(`/item/${id}`, { authenticated: true, itemTitle: "The Matrix" })).toBe(
      "The Matrix — MediaFlick",
    )
  })

  test("anonymous users keep settings titles, matching the real anonymous route", () => {
    expect(windowTitle("/settings/appearance", { authenticated: false })).toBe("Settings — MediaFlick")
  })
})
