import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, test } from "vitest"
import { LetterboxdReviewList } from "../src/components/detail/LetterboxdReviews"
import type { LetterboxdReview } from "../src/lib/api"

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

describe("Letterboxd detail reviews", () => {
  test("renders the connected member, five-star rating, review, and canonical entry link", () => {
    render(<LetterboxdReviewList reviews={[review()]} />)

    expect(screen.getByRole("heading", { name: "Alice Film Fan" })).not.toBeNull()
    expect(screen.getByLabelText("4.5 out of 5 stars from Alice Film Fan").textContent).toContain("4.5/ 5")
    expect(screen.getByText("Smart, stylish, and still startling.")).not.toBeNull()
    expect(screen.getByRole("link", { name: /open on letterboxd/i }).getAttribute("href")).toBe(
      "https://letterboxd.com/alice/film/the-matrix/",
    )
  })

  test("shows ratings without inventing review text", () => {
    const { container } = render(
      <LetterboxdReviewList reviews={[review({ review: null, entryUrl: null })]} />,
    )

    expect(screen.getByLabelText("4.5 out of 5 stars from Alice Film Fan")).not.toBeNull()
    expect(container.querySelector("blockquote")).toBeNull()
    expect(screen.getByRole("link", { name: /open on letterboxd/i }).getAttribute("href")).toBe(
      "https://letterboxd.com/alice/",
    )
  })

  test("collapses long reviews until the reader asks for the rest", () => {
    const longReview = "A very considered reaction. ".repeat(30)
    const { container } = render(<LetterboxdReviewList reviews={[review({ review: longReview })]} />)

    const quote = container.querySelector("blockquote")!
    expect(quote.classList.contains("line-clamp-5")).toBe(true)
    const toggle = screen.getByRole("button", { name: "Show more" })
    fireEvent.click(toggle)
    expect(toggle.textContent).toContain("Show less")
    expect(toggle.getAttribute("aria-expanded")).toBe("true")
    expect(quote.classList.contains("line-clamp-5")).toBe(false)
  })
})
