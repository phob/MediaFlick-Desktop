import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, test, vi } from "vitest"
import { ExternalLinksMenu } from "../src/components/detail/ExternalLinksMenu"

describe("external information menu", () => {
  test("renders discovered-title destinations as external links", () => {
    render(
      <ExternalLinksMenu links={[
        { id: "imdb", label: "IMDb", href: "https://www.imdb.com/title/tt0133093/" },
      ]} />,
    )

    fireEvent.pointerDown(screen.getByRole("button", { name: "More info" }), { button: 0 })
    const link = screen.getByRole("menuitem", { name: "View on IMDb" })
    expect(link.getAttribute("href")).toBe("https://www.imdb.com/title/tt0133093/")
    expect(link.getAttribute("target")).toBe("_blank")
    expect(link.getAttribute("rel")).toBe("noreferrer")
  })

  test("labels Rotten Tomatoes search links as searches", () => {
    render(
      <ExternalLinksMenu links={[{
        id: "rotten-tomatoes-search",
        label: "Rotten Tomatoes",
        actionLabel: "Search Rotten Tomatoes",
        href: "https://www.rottentomatoes.com/search?search=The%20Matrix%201999",
      }]} />,
    )

    fireEvent.pointerDown(screen.getByRole("button", { name: "More info" }), { button: 0 })
    const link = screen.getByRole("menuitem", { name: "Search Rotten Tomatoes" })
    expect(link.getAttribute("href")).toBe(
      "https://www.rottentomatoes.com/search?search=The%20Matrix%201999",
    )
  })

  test("hands library-title selections to the native opener", () => {
    const onSelect = vi.fn()
    render(<ExternalLinksMenu links={[{ id: "tmdb", label: "TMDB" }]} onSelect={onSelect} />)

    fireEvent.pointerDown(screen.getByRole("button", { name: "More info" }), { button: 0 })
    fireEvent.click(screen.getByRole("menuitem", { name: "View on TMDB" }))
    expect(onSelect).toHaveBeenCalledWith("tmdb")
  })
})
