import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test, vi } from "vitest"
import { PreviewProvider } from "../src/components/PreviewCard"
import { EpisodeGrid, SeasonBrowser } from "../src/components/detail/SeasonBrowser"
import { seasonRailOrder } from "../src/lib/seasons"
import { requireElement, itemSummary } from "./support/fixtures"

function withProviders(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  )
}

const specials = itemSummary({ id: "season-0", kind: "Season", name: "Specials", indexNumber: 0 })
const seasonOne = itemSummary({ id: "season-1", kind: "Season", name: "Season 1", indexNumber: 1 })
const seasonTwo = itemSummary({ id: "season-2", kind: "Season", name: "Season 2", indexNumber: 2 })

function episode(id: string, index: number, overrides: Partial<Parameters<typeof itemSummary>[0]> = {}) {
  return itemSummary({
    id,
    kind: "Episode",
    name: `Episode name ${index}`,
    indexNumber: index,
    parentIndexNumber: 1,
    seasonId: "season-1",
    runtimeTicks: 33_000_000_000,
    overview: "A synopsis that must not appear on the card.",
    ...overrides,
  })
}

describe("season browser", () => {
  test("orders Specials after the regular seasons", () => {
    expect(seasonRailOrder([specials, seasonOne, seasonTwo]).map((season) => season.id)).toEqual([
      "season-1",
      "season-2",
      "season-0",
    ])
  })

  test("the season rail marks the selection and reports a season click", () => {
    const onSelect = vi.fn()
    withProviders(
      <SeasonBrowser
        seasons={[seasonOne, seasonTwo, specials]}
        selectedSeason={seasonOne}
        onSelect={onSelect}
        episodes={[episode("episode-1", 1)]}
        episodesPending={false}
        episodesError={null}
        onRetry={vi.fn()}
        nextUpEpisodeId={null}
      />,
    )

    const selected = screen.getByRole("button", { name: /Season 1/ })
    expect(selected.getAttribute("aria-pressed")).toBe("true")
    const other = screen.getByRole("button", { name: /Season 2/ })
    expect(other.getAttribute("aria-pressed")).toBe("false")
    fireEvent.click(other)
    expect(onSelect).toHaveBeenCalledWith(seasonTwo)
  })

  test("episode cards keep number, title, runtime, and progress but no synopsis", () => {
    const watching = episode("episode-2", 2, {
      positionTicks: 16_500_000_000,
      communityRating: 8.4,
    })
    const { container } = withProviders(
      <EpisodeGrid episodes={[episode("episode-1", 1), watching]} parentId="season-1" />,
    )

    expect(screen.getByText("Episode name 2")).toBeTruthy()
    expect(screen.getByText("2.")).toBeTruthy()
    expect(screen.getAllByText("55m")).toHaveLength(2)
    expect(screen.getByLabelText("Jellyfin community rating 8.4 out of 10")).toBeTruthy()
    expect(screen.queryByText(/A synopsis/)).toBeNull()
    const bar = requireElement(
      container.querySelector<HTMLElement>("li .h-full.bg-primary"),
      "resume progress bar",
    )
    expect(bar.style.width).toBe("50%")
  })

  test("episode cards expose the complete inline action set when previews are off", () => {
    withProviders(
      <PreviewProvider enabled={false}>
        <EpisodeGrid episodes={[episode("episode-1", 1)]} parentId="season-1" />
      </PreviewProvider>,
    )

    expect(screen.getByRole("button", { name: "Play" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Add to My List" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Mark as watched" })).toBeTruthy()
  })

  test("the next-up episode carries the accent marker", () => {
    const { container } = withProviders(
      <EpisodeGrid
        episodes={[episode("episode-1", 1), episode("episode-2", 2)]}
        parentId="season-1"
        nextUpEpisodeId="episode-2"
      />,
    )

    const marked = requireElement(
      container.querySelector<HTMLElement>("[data-next-up]"),
      "next-up episode card",
    )
    expect(marked.textContent).toContain("Episode name 2")
    expect(marked.textContent).toContain("(Next up)")
    expect(container.querySelectorAll("[data-next-up]")).toHaveLength(1)
  })

  test("an empty season says so instead of rendering a bare grid", () => {
    withProviders(
      <SeasonBrowser
        seasons={[seasonOne]}
        selectedSeason={seasonOne}
        onSelect={vi.fn()}
        episodes={[]}
        episodesPending={false}
        episodesError={null}
        onRetry={vi.fn()}
        nextUpEpisodeId={null}
      />,
    )
    expect(screen.getByText("This season has no episodes.")).toBeTruthy()
  })

  test("a failed episode fetch offers a retry without losing the rail", () => {
    const onRetry = vi.fn()
    withProviders(
      <SeasonBrowser
        seasons={[seasonOne, seasonTwo]}
        selectedSeason={seasonTwo}
        onSelect={vi.fn()}
        episodes={[]}
        episodesPending={false}
        episodesError={new Error("boom")}
        onRetry={onRetry}
        nextUpEpisodeId={null}
      />,
    )

    expect(screen.getByRole("button", { name: /Season 1/ })).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Try again" }))
    expect(onRetry).toHaveBeenCalled()
  })
})
