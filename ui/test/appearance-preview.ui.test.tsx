import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, within } from "@testing-library/react"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, RatingsIntegrationStatus } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import { Appearance } from "@/routes/Settings"
import { itemSummary, requireElement } from "./support/fixtures"

const appearance = (cardPreviews: boolean): ClientSettings["appearance"] => ({
  theme: "light",
  accent: "cobalt",
  density: "comfortable",
  artworkIntensity: 80,
  backdropIntensity: 60,
  reducedMotion: false,
  cardPreviews,
  showMediaInfo: true,
  ratingSources: [],
})

const settings = (cardPreviews: boolean): ClientSettings => ({
  client: {
    player: {
      playerBackend: "mpv",
      libmpvProfile: "standard",
      mpvPath: null,
      mpchcPath: null,
      defaultFullscreen: "windowed",
      markWatchedNext: null,
      playerConfigured: true,
    },
    playback: {
      streamingQuality: "original",
      skipIntro: "disabled",
      skipCredits: "disabled",
      skipRecap: "disabled",
      skipCommercial: "disabled",
    },
    application: { closeBehavior: "exit_app", showScrollbars: false, logLevel: "info" },
  },
  appearance: appearance(cardPreviews),
  capabilities: { platform: "windows", libmpv: true, mpchc: false, mpvInstaller: false },
  streamingQuality: "original",
  playerBackend: "mpv",
  playerConfigured: true,
  serverUrl: null,
})

const ratingsStatus: RatingsIntegrationStatus = {
  boundaryVersion: 1,
  effectiveOrigin: "none",
  available: false,
  selectionEnabled: true,
  plugin: { available: false, capability: "ratings-v1", boundaryVersion: 1, detail: "" },
  sources: [],
  selectedSources: [],
}

const movie = itemSummary({ id: "movie-1", kind: "Movie", name: "The Matrix", year: 1999 })

function LocationProbe() {
  const location = useLocation()
  return <output data-location>{location.pathname}</output>
}

function renderAppearance(cardPreviews: boolean, authenticated = true) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  client.setQueryData(queryKeys.settings, settings(cardPreviews))
  client.setQueryData(queryKeys.status, { authenticated })
  client.setQueryData(queryKeys.home, {
    rows: [
      { id: "resume", title: "Continue Watching", items: [] },
      { id: "recent", title: "Recently Added", items: [movie] },
    ],
  })
  client.setQueryData(queryKeys.ratingsStatus, ratingsStatus)
  const requests: string[] = []
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    requests.push(String(input))
    return new Response(JSON.stringify({}), { status: 200 })
  }))
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/settings/appearance"]}>
        <Appearance />
        <LocationProbe />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return requests
}

function location() {
  return document.querySelector("[data-location]")?.textContent
}

function hoverWithMouse(element: Element) {
  const event = new MouseEvent("pointerover", { bubbles: true })
  Object.defineProperty(event, "pointerType", { value: "mouse" })
  fireEvent(element, event)
}

function shelfCard() {
  return requireElement(
    document.querySelector(".appearance-preview-shelf .signal-card"),
    "a preview shelf card",
  )
}

function restOnCard() {
  act(() => {
    hoverWithMouse(shelfCard())
    vi.advanceTimersByTime(550)
  })
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe("appearance settings live preview", () => {
  test("requires a Jellyfin account before showing account-owned settings", () => {
    renderAppearance(true, false)

    expect(screen.getByText("Sign in required")).not.toBeNull()
    expect(screen.queryByRole("heading", { name: "Live preview" })).toBeNull()
  })

  test("opens the real expanded panel after resting the pointer on a preview card", () => {
    renderAppearance(true)
    restOnCard()

    const panel = requireElement(
      document.querySelector(".preview-panel"),
      "expanded media-card preview",
    )
    // The panel is themed by the unsaved draft choices, like the shelf itself.
    expect(panel.closest("[data-theme='light']")).not.toBeNull()
    expect(panel.closest("[data-accent='cobalt']")).not.toBeNull()
    // It portals outside the page's paint containment, exactly where the
    // app's own panel lives.
    expect(panel.closest(".appearance-preview")).toBeNull()
    expect(panel.closest(".content-viewport")).toBeNull()
    expect(panel.parentElement?.parentElement).toBe(document.body)
  })

  test("keeps the panel's state-changing actions inert while hovering for real", () => {
    const requests = renderAppearance(true)
    restOnCard()

    const panel = requireElement(
      document.querySelector<HTMLElement>(".preview-panel"),
      "expanded media-card preview",
    )
    fireEvent.click(within(panel).getByRole("button", { name: "Play" }))
    fireEvent.click(within(panel).getByRole("button", { name: "Add to My List" }))
    fireEvent.click(within(panel).getByRole("button", { name: "Mark as watched" }))

    expect(requests.some((path) => path.includes("/api/play"))).toBe(false)
    expect(requests.some((path) => path.includes("/favorite"))).toBe(false)
    expect(requests.some((path) => path.includes("/played"))).toBe(false)
  })

  test("navigates to the item's details exactly like a live card's panel", () => {
    renderAppearance(true)
    restOnCard()

    const panel = requireElement(
      document.querySelector(".preview-panel"),
      "expanded media-card preview",
    )
    fireEvent.click(panel)

    expect(location()).toBe("/item/movie-1")
  })

  test("leaves the cards without a panel when previews are disabled", () => {
    renderAppearance(false)

    expect(screen.queryByRole("button", { name: "Play" })).toBeNull()
    restOnCard()

    expect(document.querySelector(".preview-panel")).toBeNull()
    // The quick actions stay on the card instead, revealed by the same hover.
    expect(document.querySelector(".appearance-preview-shelf .card-inline-actions")).not.toBeNull()
  })
})
