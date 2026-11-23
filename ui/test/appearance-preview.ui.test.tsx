import { api } from "@/lib/api"
import { DEFAULT_VIEWING } from "@/lib/viewing"
import { act, fireEvent, render, screen, within, waitFor } from "@testing-library/react"
import { useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, ItemSummary, RatingsIntegrationStatus } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import { Appearance } from "@/routes/settings/AppearanceSettings"
import { appStatus, itemSummary, requireElement } from "./support/fixtures"
import { clientSettingsFixture } from "./support/settings"
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

const settings = (cardPreviews: boolean): ClientSettings => {
  const base = clientSettingsFixture()
  return {
    ...base,
    client: {
      player: {
        ...base.client.player,
        playerBackend: "mpv",
        defaultFullscreen: "windowed",
        markWatchedNext: null,
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
    capabilities: { ...base.capabilities, mpvInstaller: false },
    serverUrl: null,
  }
}

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
const resumeMovie = itemSummary({
  id: "movie-2",
  kind: "Movie",
  name: "Blade Runner",
  year: 1982,
  positionTicks: 12_000_000_000,
  runtimeTicks: 70_000_000_000,
})

function LocationProbe() {
  const location = useLocation()
  return <output data-testid="appearance-location">{location.pathname}</output>
}

function renderAppearance(cardPreviews: boolean, continueWatching: ItemSummary[] = []) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.settings, settings(cardPreviews))
  client.setQueryData(queryKeys.viewing("anonymous:anonymous"), DEFAULT_VIEWING)
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true }))
  client.setQueryData(queryKeys.home, {
    continueWatching,
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
    fireEvent.click(within(panel).getByRole("button", { name: "Add to favorites" }))
    fireEvent.click(within(panel).getByRole("button", { name: "Mark watched" }))

    expect(requests.some((path) => path.includes("/api/play"))).toBe(false)
    expect(requests.some((path) => path.includes("/favorite"))).toBe(false)
    expect(requests.some((path) => path.includes("/played"))).toBe(false)
  })

  test("renders preview shelf cards declaratively without links, buttons, or inert DOM mutations", () => {
    const requests = renderAppearance(false, [resumeMovie])
    const shelf = requireElement(
      document.querySelector<HTMLElement>(".appearance-preview-shelf"),
      "appearance preview shelf",
    )

    expect(shelf.querySelectorAll("a")).toHaveLength(0)
    expect(shelf.querySelectorAll("button")).toHaveLength(0)
    expect(shelf.querySelectorAll("[inert]")).toHaveLength(0)

    const inlineActions = shelf.querySelectorAll(".card-inline-actions .preview-action")
    expect(inlineActions.length).toBeGreaterThanOrEqual(6)

    for (const action of inlineActions) {
      fireEvent.click(action)
    }
    for (const target of [
      ...within(shelf).getAllByText("The Matrix"),
      ...within(shelf).getAllByText("Blade Runner"),
    ]) {
      fireEvent.click(target)
    }

    expect(screen.getByTestId("appearance-location").textContent).toBe("/settings/appearance")
    expect(requests.some((path) => path.includes("/api/play"))).toBe(false)
    expect(requests.some((path) => path.includes("/favorite"))).toBe(false)
    expect(requests.some((path) => path.includes("/played"))).toBe(false)
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
  const save = vi.spyOn(api, "saveViewing").mockImplementation(async (value) => ({...DEFAULT_VIEWING, ...value}))
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({previewDelayMs:850, textScale:125})))
  await waitFor(() => expect((screen.getByRole("button", {name:"Save"}) as HTMLButtonElement).disabled).toBe(true))
  fireEvent.click(screen.getByRole("button", {name:"Restore defaults"}))
  expect(input.value).toBe("550")
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect(input.value).toBe("850")
})
