import { act, fireEvent, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { MediaCard } from "../src/components/MediaCard"
import { PreviewProvider } from "../src/components/PreviewCard"
import type { ItemDetail, ItemSummary } from "../src/lib/api"

const mutations = vi.hoisted(() => ({
  favorite: vi.fn(),
  play: vi.fn(),
  played: vi.fn(),
}))

vi.mock("@/lib/queries", () => ({
  useItem: (id: string | undefined) => ({
    data: id
      ? ({
          id,
          genres: ["Science Fiction"],
        } as ItemDetail)
      : undefined,
  }),
  useNextUp: () => ({ data: undefined }),
  usePlay: () => ({ isPending: false, mutate: mutations.play }),
  useSetFavorite: () => ({ isPending: false, mutate: mutations.favorite }),
  useSetPlayed: () => ({ isPending: false, mutate: mutations.played }),
}))

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

function LocationProbe() {
  const location = useLocation()
  return <output data-location>{location.pathname}</output>
}

function Providers({ children }: { children: ReactNode }) {
  return (
    <MemoryRouter initialEntries={["/home"]}>
      <PreviewProvider>{children}</PreviewProvider>
      <LocationProbe />
    </MemoryRouter>
  )
}

function location() {
  return document.querySelector("[data-location]")?.textContent
}

function hoverWithMouse(element: Element) {
  const event = new MouseEvent("pointerover", { bubbles: true })
  Object.defineProperty(event, "pointerType", { value: "mouse" })
  fireEvent(element, event)
}

function renderOpenPreview() {
  render(<MediaCard item={movie} />, { wrapper: Providers })
  const card = screen.getAllByRole("link")[0]!

  act(() => {
    hoverWithMouse(card)
    vi.advanceTimersByTime(550)
  })

  const panel = document.querySelector(".preview-panel")
  expect(panel).not.toBeNull()
  return panel!
}

beforeEach(() => {
  vi.useFakeTimers()
  mutations.favorite.mockReset()
  mutations.play.mockReset()
  mutations.played.mockReset()
})

afterEach(() => vi.useRealTimers())

describe("expanded media-card details target", () => {
  test("opens item details when the panel background is clicked", () => {
    const panel = renderOpenPreview()

    fireEvent.click(panel)

    expect(location()).toBe("/item/movie-1")
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
