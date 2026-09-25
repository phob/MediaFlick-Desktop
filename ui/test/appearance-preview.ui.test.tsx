import { api } from "@/lib/api"
import { DEFAULT_COMFORT, DEFAULT_VIEWING } from "@/lib/viewing"
import { act, fireEvent, render, screen, within, waitFor } from "@testing-library/react"
import { useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, RatingsIntegrationStatus } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import { Appearance } from "@/routes/settings/AppearanceSettings"
import { itemSummary, requireElement } from "./support/fixtures"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const appearance = (cardPreviews: boolean): ClientSettings["appearance"] => ({
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
      defaultFullscreen: "windowed",
      markWatchedNext: null,
      playerConfigured: true, comfort: DEFAULT_COMFORT,
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
  capabilities: { platform: "windows", libmpv: true, integratedLibmpvOverlay: false, mpvInstaller: false },
  recoveries: [],
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

function renderAppearance(cardPreviews: boolean) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.settings, settings(cardPreviews))
  client.setQueryData(queryKeys.viewing("anonymous:anonymous"), DEFAULT_VIEWING)
  client.setQueryData(queryKeys.status, { authenticated: true })
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

test("appearance slider announces its value as a percentage", () => {
  renderAppearance(false)
  const slider = screen.getByRole("slider", { name: "Artwork intensity slider" })
  expect(slider.getAttribute("aria-valuetext")).toBe("80 percent")
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
