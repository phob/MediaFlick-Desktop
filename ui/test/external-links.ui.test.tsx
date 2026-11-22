import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, test, vi } from "vitest"
import { ExternalLinksMenu } from "@/components/detail/ExternalLinksMenu"
import type { ExternalLink } from "@/lib/api"

describe("ExternalLinksMenu", () => {
  test("renders nothing when Rust supplies no external links", () => {
    const { container } = render(<ExternalLinksMenu links={[]} />)
    expect(container.innerHTML).toBe("")
  })

  test("renders provider action items and direct url items from server descriptors", async () => {
    const onSelect = vi.fn()
    const links: ExternalLink[] = [
      { kind: "provider", id: "imdb", label: "IMDb" },
      {
        kind: "url",
        id: "tmdb",
        label: "TMDB",
        href: "https://www.themoviedb.org/movie/603",
      },
      {
        kind: "url",
        id: "rotten-tomatoes-search",
        label: "Rotten Tomatoes",
        actionLabel: "Search Rotten Tomatoes",
        href: "https://www.rottentomatoes.com/search?search=The%20Matrix%201999",
      },
    ]

    render(<ExternalLinksMenu links={links} onSelect={onSelect} />)
    fireEvent.pointerDown(screen.getByRole("button", { name: "More info" }))

    const imdb = await screen.findByRole("menuitem", { name: "View on IMDb" })
    const tmdb = screen.getByRole("menuitem", { name: "View on TMDB" })
    const rt = screen.getByRole("menuitem", { name: "Search Rotten Tomatoes" })

    expect(tmdb.getAttribute("href")).toBe("https://www.themoviedb.org/movie/603")
    expect(rt.getAttribute("href")).toBe(
      "https://www.rottentomatoes.com/search?search=The%20Matrix%201999",
    )

    fireEvent.click(imdb)
    expect(onSelect).toHaveBeenCalledWith("imdb")
  })
})
