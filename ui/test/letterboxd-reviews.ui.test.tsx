import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import {
  LetterboxdReviewList,
  LetterboxdReviews,
  type LetterboxdQueries,
} from "../src/components/detail/LetterboxdReviews"
import type { LetterboxdReview } from "../src/lib/api"
import { itemDetail } from "./support/fixtures"

const itemLookup = vi.fn<LetterboxdQueries["item"]>()
const movieLookup = vi.fn<LetterboxdQueries["movie"]>()
const queries: LetterboxdQueries = { item: itemLookup, movie: movieLookup }

function review(overrides: Partial<LetterboxdReview> = {}): LetterboxdReview {
  return {
    profileId: "profile-1",
    username: "alice",
    displayName: "Alice Film Fan",
    profileUrl: "https://letterboxd.com/alice/",
    entryUrl: "https://letterboxd.com/alice/film/the-matrix/",
    rating: 4.5,
    review: "Smart, stylish, and still startling.",
    reviewTruncated: false,
    watchedDate: "2026-08-04",
    stale: false,
    ...overrides,
  }
}

const movie = itemDetail({
  id: "movie-1",
  name: "The Matrix",
  kind: "Movie",
  providerIds: { tmdb: "603", imdb: null, tvdb: null },
})

function reviewPreview() {
  return screen.queryByRole("dialog", { name: "Alice Film Fan's Letterboxd review" })
}

beforeEach(() => {
  itemLookup.mockReset()
  movieLookup.mockReset()
  itemLookup.mockReturnValue({ isPending: false, error: null, data: undefined })
  movieLookup.mockReturnValue({ isPending: false, error: null, data: undefined })
})

afterEach(() => {
  vi.useRealTimers()
})

describe("Letterboxd detail activity", () => {
  test("opens the written review on mouse hover", () => {
    vi.useFakeTimers()
    render(<LetterboxdReviewList reviews={[review()]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })

    fireEvent.pointerEnter(tile, { pointerType: "mouse" })
    act(() => vi.advanceTimersByTime(1_000))
    expect(reviewPreview()?.textContent).toContain("Smart, stylish, and still startling.")
  })

  test("lets keyboard focus enter the review link and Escape return to the profile", () => {
    render(<LetterboxdReviewList reviews={[review()]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })

    fireEvent.focus(tile)
    const reviewLink = screen.getByRole("link", { name: "Read Alice Film Fan's review on Letterboxd" })
    fireEvent.keyDown(tile, { key: "Tab" })
    expect(document.activeElement).toBe(reviewLink)

    fireEvent.keyDown(reviewLink, { key: "Escape" })
    expect(document.activeElement).toBe(tile)
    expect(reviewPreview()).toBeNull()
  })

  test("keeps available profiles and reports a partial refresh failure", () => {
    itemLookup.mockReturnValue({
      isPending: false,
      error: null,
      data: {
        reviews: [review({ stale: true })],
        configuredProfiles: 2,
        unavailableProfiles: 1,
      },
    })
    render(<LetterboxdReviews item={movie} queries={queries} />)

    expect(screen.getByRole("status")).not.toBeNull()
    expect(screen.getByRole("link", { name: /Alice Film Fan/i })).not.toBeNull()
  })
})
