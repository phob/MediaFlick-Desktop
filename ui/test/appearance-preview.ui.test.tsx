import { api } from "@/lib/api"
import { DEFAULT_COMFORT, DEFAULT_VIEWING } from "@/lib/viewing"
import { act, fireEvent, render, screen, within, waitFor } from "@testing-library/react"
import { Route, Routes, useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, RatingsIntegrationStatus } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import Settings, { Appearance } from "@/routes/Settings"
import { itemSummary, requireElement } from "./support/fixtures"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

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
      mpvPath: null,
      mpchcPath: null,
      defaultFullscreen: "windowed",
      markWatchedNext: null,
      playerConfigured: true,
    },
    playback: { comfort: DEFAULT_COMFORT,
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
  const client = testQueryClient()
  client.setQueryData(queryKeys.settings, settings(cardPreviews))
  client.setQueryData(["viewing", "anonymous:anonymous"], DEFAULT_VIEWING)
  client.setQueryData(queryKeys.status, { authenticated })
  client.setQueryData(queryKeys.home, {
    continueWatching: [],
    rows: [{ kind: "builtIn", id: "recentlyAdded", title: "Recently Added", items: [movie] }],
  })
  client.setQueryData(queryKeys.ratingsStatus, ratingsStatus)
  const requests: string[] = []
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    requests.push(String(input))
    return new Response(JSON.stringify({}), { status: 200 })
  }))
  render(
    <TestProviders client={client} initialEntries={["/settings/appearance"]}>
        <Appearance />
        <LocationProbe />
    </TestProviders>,
  )
  return requests
}

function location() {
  return document.querySelector("[data-location]")?.textContent
}

test("appearance sliders expose names, descriptions, and percentage values", () => {
  renderAppearance(false)
  for (const [name, value] of [["Artwork intensity", "80 percent"], ["Backdrop intensity", "60 percent"]]) {
    const slider = screen.getByRole("slider", { name })
    expect(slider.getAttribute("aria-valuetext")).toBe(value)
    expect(document.getElementById(slider.getAttribute("aria-describedby") ?? "")?.textContent).toContain("%")
  }
})

test.each(["mpv", "mpchc"] as const)("%s player fields are associated with visible labels and help", (backend) => {
  const client = testQueryClient()
  const configured = settings(false)
  configured.client.player.playerBackend = backend
  configured.capabilities.mpchc = true
  client.setQueryData(queryKeys.settings, configured)
  client.setQueryData(queryKeys.status, { authenticated: true })
  render(<TestProviders client={client} initialEntries={["/settings/client/player"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)
  const names = backend === "mpv" ? ["Mark watched key", "mpv executable"] : ["MPC-HC executable"]
  for (const name of names) {
    const input = screen.getByRole("textbox", { name })
    expect(document.getElementById(input.getAttribute("aria-describedby") ?? "")?.textContent).toBeTruthy()
  }
})

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


test("preview delay shares the preview toggle's Save, Reset, and Discard workflow", async () => {
  vi.useRealTimers()
  renderAppearance(false)
  const input = screen.getByRole("spinbutton", {name:"Card preview delay"}) as HTMLInputElement
  expect(input.disabled).toBe(true)
  fireEvent.click(screen.getByRole("switch", {name:"Show pop-out previews on cards"}))
  expect(input.disabled).toBe(false)
  fireEvent.change(input, {target:{value:"850"}})
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect(input.value).toBe("550")
  expect(input.disabled).toBe(true)
  fireEvent.click(screen.getByRole("switch", {name:"Show pop-out previews on cards"}))
  fireEvent.change(input, {target:{value:"850"}})
  vi.spyOn(api.settingsPatch, "appearance").mockImplementation(async (appearance) => ({...settings(true), appearance: {...settings(true).appearance, ...appearance}}))
  vi.spyOn(api, "viewing").mockResolvedValue({...DEFAULT_VIEWING, textScale:125})
  const save = vi.spyOn(api, "saveViewing").mockImplementation(async (value) => value)
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({previewDelayMs:850, textScale:125})))
  await waitFor(() => expect((screen.getByRole("button", {name:"Save"}) as HTMLButtonElement).disabled).toBe(true))
  fireEvent.click(screen.getByRole("button", {name:"Reset"}))
  expect(input.value).toBe("550")
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect(input.value).toBe("850")
})
