import { act, fireEvent, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { MediaCard } from "../src/components/MediaCard"
import { PreviewProvider, type PreviewDependencies } from "../src/components/PreviewCard"
import type {
  ItemRatings,
  ItemSummary,
  MediaStream,
  RatingSourceDefinition,
} from "../src/lib/api"
import { RatingsContext } from "../src/lib/rating-context"
import { TechnicalContext } from "../src/lib/technical-context"
import { itemDetail, requireElement } from "./support/fixtures"

const mutations = {
  favorite: vi.fn<(input: { id: string; favorite: boolean }) => void>(),
  play: vi.fn<(input: { id: string; resume: boolean }) => void>(),
  played: vi.fn<(input: { id: string; played: boolean; context?: string | null }) => void>(),
}

const dependencies: PreviewDependencies = {
  item: (id) => ({
    data: id
      ? itemDetail({
          id,
          name: "The Matrix",
          kind: "Movie",
          genres: ["Science Fiction"],
        })
      : undefined,
  }),
  nextUp: () => ({ data: undefined }),
  play: () => ({ isPending: false, mutate: mutations.play }),
  favorite: () => ({ isPending: false, mutate: mutations.favorite }),
  played: () => ({ isPending: false, mutate: mutations.played }),
}

function stream(overrides: Partial<MediaStream>): MediaStream {
  return {
    index: 0,
    type: null,
    codec: null,
    profile: null,
    language: null,
    title: null,
    displayTitle: null,
    width: null,
    height: null,
    channels: null,
    audioSpatialFormat: null,
    videoRange: null,
    videoRangeType: null,
    bitDepth: null,
    isDefault: false,
    isForced: false,
    isHearingImpaired: false,
    isExternal: false,
    ...overrides,
  }
}

const letterboxd: RatingSourceDefinition = {
  id: "letterboxd",
  label: "Letterboxd",
  shortLabel: "LB",
  scaleMax: 5,
  format: "stars",
  known: true,
}

const movieRatings: ItemRatings = {
  id: "movie-1",
  ratings: [{
    sourceId: "letterboxd",
    rawSource: "letterboxd",
    value: 4.2,
    score: 84,
    votes: 1_000,
    scaleMax: 5,
  }],
  origin: "local_mdblist",
  fetchedAt: 1,
  sourceUpdatedAt: null,
  stale: false,
  schemaVersion: 1,
}

const movie: ItemSummary = {
  id: "movie-1",
  kind: "Movie",
  name: "The Matrix",
  year: 1999,
  runtimeTicks: 81600000000,
  communityRating: 8.2,
  officialRating: "R",
  seriesId: null,
  seriesName: null,
  indexNumber: null,
  parentIndexNumber: null,
  primaryImageTag: null,
  thumbImageTag: null,
  logoImageTag: null,
  backdropImageTag: null,
  childCount: null,
  premiereDate: null,
  seasonId: null,
  played: false,
  playCount: 0,
  positionTicks: 0,
  favorite: false,
}

// Technical streams no longer travel on summaries; the preview reads them
// from the live batched technical channel, keyed by item id.
const movieStreams: MediaStream[] = [
  stream({ type: "Video", codec: "hevc", width: 3840, height: 1608, videoRangeType: "DOVI" }),
  stream({ index: 1, type: "Audio", codec: "truehd", channels: 8, audioSpatialFormat: "DolbyAtmos" }),
]

function LocationProbe() {
  const location = useLocation()
  return <output data-location>{location.pathname}</output>
}

function Providers({ children }: { children: ReactNode }) {
  return (
    <MemoryRouter initialEntries={["/home"]}>
      <RatingsContext.Provider value={{
        items: new Map([[movie.id, movieRatings]]),
        selected: [letterboxd.id],
        definitions: new Map([[letterboxd.id, letterboxd]]),
        register: () => () => {},
      }}>
        <TechnicalContext.Provider value={{
          items: new Map([[movie.id, movieStreams]]),
          register: () => () => {},
        }}>
          <PreviewProvider dependencies={dependencies}>{children}</PreviewProvider>
          <LocationProbe />
        </TechnicalContext.Provider>
      </RatingsContext.Provider>
    </MemoryRouter>
  )
}

function location() {
  return document.querySelector("[data-location]")?.textContent
}

function hoverWithMouse(element: Element, init?: MouseEventInit) {
  const event = new MouseEvent("pointerover", { bubbles: true, ...init })
  Object.defineProperty(event, "pointerType", { value: "mouse" })
  fireEvent(element, event)
}

function renderOpenPreview() {
  render(<MediaCard item={movie} />, { wrapper: Providers })
  const card = screen.getAllByRole("link")[0]
  if (!card) throw new Error("Expected a media-card link")

  act(() => {
    hoverWithMouse(card)
    vi.advanceTimersByTime(550)
  })

  return requireElement(
    document.querySelector<HTMLElement>(".preview-panel"),
    "expanded media-card preview",
  )
}

beforeEach(() => {
  vi.useFakeTimers()
  mutations.favorite.mockReset()
  mutations.play.mockReset()
  mutations.played.mockReset()
})

afterEach(() => vi.useRealTimers())

describe("expanded media-card details target", () => {
  test("carries the configured rating and technical media facts into the larger preview", () => {
    const panel = renderOpenPreview()
    const rating = panel.querySelector(".card-rating-readout")
    const technical = panel.querySelector(".preview-technical-readout")

    expect(rating?.querySelector("[data-rating-source-icon='letterboxd']")).toBeTruthy()
    expect(rating?.textContent).toContain("★4.2")
    expect(technical?.getAttribute("aria-label")).toContain("Technical media information")
    expect(technical?.textContent).toContain("4K")
    expect(technical?.textContent).toContain("DV")
    expect(technical?.textContent).toContain("TrueHD")
    expect(technical?.textContent).toContain("Atmos")
  })

  test("opens item details when the panel background is clicked", () => {
    const panel = renderOpenPreview()

    fireEvent.click(panel)

    expect(location()).toBe("/item/movie-1")
  })

  test("treats a release from a press that began on the card as the card click", () => {
    // The press straddled the panel's appearance: mousedown hit the card, the
    // panel mounted during the 170ms fade-in, and mouseup lands on the panel.
    // The browser fires no click for a down/up target mismatch, so the panel
    // must honour the release itself.
    const panel = renderOpenPreview()

    fireEvent.pointerUp(panel, { button: 0 })

    expect(location()).toBe("/item/movie-1")
  })

  test("leaves presses that began inside the panel to their own click", () => {
    const panel = renderOpenPreview()

    fireEvent.pointerDown(panel, { button: 0 })
    fireEvent.pointerUp(panel, { button: 0 })

    expect(location()).toBe("/home")
  })

  test("does not arm the preview under a pointer that is mid-drag", () => {
    render(<MediaCard item={movie} />, { wrapper: Providers })
    const card = screen.getAllByRole("link")[0]
    if (!card) throw new Error("Expected a media-card link")

    act(() => {
      hoverWithMouse(card, { buttons: 1 })
      vi.advanceTimersByTime(550)
    })

    expect(document.querySelector(".preview-panel")).toBeNull()
  })

  test("exposes a keyboard-focusable details link across the complete panel", () => {
    renderOpenPreview()
    const details = screen.getByRole("link", { name: "Open details for The Matrix" })

    act(() => details.focus())
    expect(document.activeElement).toBe(details)
    fireEvent.click(details)

    expect(location()).toBe("/item/movie-1")
    expect(screen.queryByRole("button", { name: "More info" })).toBeNull()
  })

  test("keeps each action control isolated from details navigation", () => {
    renderOpenPreview()

    fireEvent.click(screen.getByRole("button", { name: "Play" }))
    fireEvent.click(screen.getByRole("button", { name: "Add to My List" }))
    fireEvent.click(screen.getByRole("button", { name: "Mark as watched" }))

    expect(mutations.play).toHaveBeenCalledWith({ id: "movie-1", resume: false })
    expect(mutations.favorite).toHaveBeenCalledWith({ id: "movie-1", favorite: true })
    expect(mutations.played).toHaveBeenCalledWith({
      id: "movie-1",
      played: true,
      context: null,
    })
    expect(location()).toBe("/home")
  })
})
