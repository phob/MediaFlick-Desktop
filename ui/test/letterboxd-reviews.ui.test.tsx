import { act, createEvent, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import {
  DiscoverLetterboxdReviews,
  LetterboxdReviewList,
  LetterboxdReviews,
  LetterboxdMovieReviews,
} from "../src/components/detail/LetterboxdReviews"
import type { ItemDetail, LetterboxdReview } from "../src/lib/api"

const useLetterboxdReviews = vi.hoisted(() => vi.fn())
const useLetterboxdMovieReviews = vi.hoisted(() => vi.fn())

vi.mock("@/lib/queries", () => ({ useLetterboxdMovieReviews, useLetterboxdReviews }))

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

const movie = {
  id: "movie-1",
  kind: "Movie",
  providerIds: { tmdb: "603", imdb: null, tvdb: null },
} as ItemDetail

beforeEach(() => {
  useLetterboxdReviews.mockReset()
  useLetterboxdMovieReviews.mockReset()
})

afterEach(() => {
  vi.useRealTimers()
})

describe("Letterboxd detail activity", () => {
  test("renders connected profiles as an ordered cast-style rail", () => {
    render(
      <LetterboxdReviewList
        reviews={[
          review(),
          review({
            profileId: "profile-2",
            username: "bob",
            displayName: "Bob",
            entryUrl: null,
            profileUrl: "https://letterboxd.com/bob/",
            review: null,
            rating: 3,
          }),
        ]}
      />,
    )

    const rail = screen.getByRole("list", {
      name: "Connected Letterboxd profiles with activity for this movie",
    })
    expect(rail.className).toContain("overflow-x-auto")
    expect(screen.getAllByRole("listitem")).toHaveLength(2)
    const links = screen.getAllByRole("link")
    expect(links[0].getAttribute("href")).toBe(
      "https://letterboxd.com/alice/film/the-matrix/",
    )
    expect(links[1].getAttribute("href")).toBe("https://letterboxd.com/bob/")
    expect(rail.querySelectorAll("[data-letterboxd-mark]")).toHaveLength(2)
  })

  test("shows five star positions including an exact half-star fill", () => {
    const { container } = render(<LetterboxdReviewList reviews={[review()]} />)

    const stars = screen.getByRole("img", {
      name: "4.5 out of 5 stars from Alice Film Fan",
    })
    expect(stars.children).toHaveLength(5)
    const fills = container.querySelectorAll<HTMLElement>("[role=img] > span > span")
    expect(fills).toHaveLength(5)
    expect(fills[4].style.width).toBe("50%")
  })

  test("keeps review-only activity aligned without inventing a rating", () => {
    const { container } = render(
      <LetterboxdReviewList reviews={[review({ rating: null })]} />,
    )

    expect(screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })).not
      .toBeNull()
    expect(container.querySelector("[role=img]")).toBeNull()
    expect(container.querySelector(".h-4[aria-hidden]")).not.toBeNull()
  })

  test("opens the review from the whole tile only after the pointer delay", () => {
    vi.useFakeTimers()
    render(<LetterboxdReviewList reviews={[review()]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })

    fireEvent.pointerEnter(tile, { pointerType: "mouse" })
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()

    act(() => vi.advanceTimersByTime(349))
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()

    act(() => vi.advanceTimersByTime(1))
    const preview = document.querySelector("[data-letterboxd-review-preview]")
    expect(preview).not.toBeNull()
    expect(preview?.textContent).toContain("Smart, stylish, and still startling.")
    expect(preview?.textContent).toContain("Reviewed 2026-08-04")
  })

  test("keeps the preview open across the pointer gap and closes after its grace period", () => {
    vi.useFakeTimers()
    render(<LetterboxdReviewList reviews={[review()]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })
    fireEvent.pointerEnter(tile, { pointerType: "mouse" })
    act(() => vi.advanceTimersByTime(350))
    const preview = document.querySelector("[data-letterboxd-review-preview]")!

    fireEvent.pointerLeave(tile, { pointerType: "mouse" })
    act(() => vi.advanceTimersByTime(100))
    fireEvent.pointerEnter(preview, { pointerType: "mouse" })
    act(() => vi.advanceTimersByTime(100))
    expect(document.querySelector("[data-letterboxd-review-preview]")).not.toBeNull()

    fireEvent.pointerLeave(preview, { pointerType: "mouse" })
    act(() => vi.advanceTimersByTime(149))
    expect(document.querySelector("[data-letterboxd-review-preview]")).not.toBeNull()
    act(() => vi.advanceTimersByTime(1))
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()
  })

  test("does not arm the preview for a touch pointer", () => {
    vi.useFakeTimers()
    render(<LetterboxdReviewList reviews={[review()]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })
    const touchEnter = createEvent.pointerEnter(tile)
    Object.defineProperty(touchEnter, "pointerType", { value: "touch" })

    fireEvent(tile, touchEnter)
    act(() => vi.advanceTimersByTime(500))
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()
    expect(tile.getAttribute("href")).toBe("https://letterboxd.com/alice/film/the-matrix/")
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
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()
  })

  test("closes a focused preview on viewport movement and restores the profile focus", () => {
    render(<LetterboxdReviewList reviews={[review()]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })
    fireEvent.focus(tile)
    const reviewLink = screen.getByRole("link", { name: "Read Alice Film Fan's review on Letterboxd" })
    fireEvent.keyDown(tile, { key: "Tab" })

    fireEvent.scroll(window)
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()
    expect(document.activeElement).toBe(tile)
    expect(reviewLink.isConnected).toBe(false)
  })

  test("gives rating-only activity a direct link and no empty preview", () => {
    render(
      <LetterboxdReviewList reviews={[review({ review: null, entryUrl: null })]} />,
    )
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*rated 4.5/i })

    fireEvent.focus(tile)
    expect(tile.getAttribute("href")).toBe("https://letterboxd.com/alice/")
    expect(document.querySelector("[data-letterboxd-review-preview]")).toBeNull()
  })

  test("bounds long review previews and keeps the canonical continuation link", () => {
    const longReview = "A very considered reaction with details. ".repeat(30)
    render(<LetterboxdReviewList reviews={[review({ review: longReview })]} />)
    const tile = screen.getByRole("link", { name: /Alice Film Fan.*written review available/i })

    fireEvent.focus(tile)
    const preview = document.querySelector("[data-letterboxd-review-preview]")!
    const quote = preview.querySelector("blockquote")!
    expect(Array.from(quote.textContent ?? "").length).toBeLessThanOrEqual(421)
    expect(Array.from(quote.textContent ?? "").length).toBeGreaterThan(300)
    expect(quote.textContent?.endsWith("…")).toBe(true)
    expect(screen.getByRole("link", { name: /Read Alice Film Fan's review/i }).getAttribute("href"))
      .toBe("https://letterboxd.com/alice/film/the-matrix/")
  })

  test("uses cast-shaped skeletons only for the initial lookup", () => {
    useLetterboxdReviews.mockReturnValue({ isPending: true })
    const { container } = render(<LetterboxdReviews item={movie} />)

    expect(screen.getByRole("region", { name: "Loading Letterboxd activity" })).not.toBeNull()
    expect(container.querySelectorAll(".size-24.rounded-full")).toHaveLength(3)
  })

  test("loads the same activity rail for a discovered movie by TMDB id", () => {
    useLetterboxdMovieReviews.mockReturnValue({
      isPending: false,
      error: null,
      data: { reviews: [review()], configuredProfiles: 1, unavailableProfiles: 0 },
    })

    render(<LetterboxdMovieReviews tmdbId={603} />)

    expect(useLetterboxdMovieReviews).toHaveBeenCalledWith(603, true)
    expect(screen.getByRole("heading", { name: "Letterboxd" })).not.toBeNull()
    expect(screen.getByRole("link", { name: /Alice Film Fan/i })).not.toBeNull()
  })

  test("does not query an invalid discovered movie identity", () => {
    useLetterboxdMovieReviews.mockReturnValue({ isPending: false, error: null, data: undefined })

    const { container } = render(<LetterboxdMovieReviews tmdbId={0} />)

    expect(useLetterboxdMovieReviews).toHaveBeenCalledWith(0, false)
    expect(container.innerHTML).toBe("")
  })

  test("keeps Letterboxd's movie namespace off discovered series", () => {
    const { container } = render(<DiscoverLetterboxdReviews mediaType="tv" tmdbId={603} />)

    expect(useLetterboxdMovieReviews).not.toHaveBeenCalled()
    expect(container.innerHTML).toBe("")
  })

  test("keeps available profiles and reports a partial refresh failure once", () => {
    useLetterboxdReviews.mockReturnValue({
      isPending: false,
      error: null,
      data: {
        reviews: [review({ stale: true })],
        configuredProfiles: 2,
        unavailableProfiles: 1,
      },
    })
    render(<LetterboxdReviews item={movie} />)

    expect(screen.getByText("1 connected profile was unavailable; cached activity remains visible."))
      .not.toBeNull()
    expect(screen.getByRole("link", { name: /Alice Film Fan/i })).not.toBeNull()
  })

  test("shows one compact status when every configured profile is unavailable", () => {
    useLetterboxdReviews.mockReturnValue({
      isPending: false,
      error: null,
      data: { reviews: [], configuredProfiles: 2, unavailableProfiles: 2 },
    })
    render(<LetterboxdReviews item={movie} />)

    expect(screen.getByRole("heading", { name: "Letterboxd" })).not.toBeNull()
    expect(screen.getByRole("status").textContent).toBe(
      "Connected profiles could not be refreshed.",
    )
  })

  test("does not expose connected RSS activity on Series details", () => {
    useLetterboxdReviews.mockReturnValue({ isPending: false, error: null, data: undefined })
    const { container } = render(
      <LetterboxdReviews item={{ ...movie, kind: "Series" }} />,
    )

    expect(useLetterboxdReviews).toHaveBeenCalledWith("movie-1", false)
    expect(container.innerHTML).toBe("")
  })
})
