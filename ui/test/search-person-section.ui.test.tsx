import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { SearchPersonSection } from "../src/routes/Library"
import type { ItemSummary } from "../src/lib/api"

function item(overrides: Partial<ItemSummary> & Pick<ItemSummary, "id" | "kind" | "name">): ItemSummary {
  return {
    year: null,
    runtimeTicks: null,
    communityRating: null,
    officialRating: null,
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
    ...overrides,
  }
}

function renderSection(term: string) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <SearchPersonSection term={term} />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

afterEach(() => vi.unstubAllGlobals())

describe("search page live person section", () => {
  test("shows a resolved person's movies and series with a full cast-page link", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path.startsWith("/api/person/resolve")) {
        expect(path).toContain("name=tom+hanks")
        return new Response(
          JSON.stringify({
            person: {
              jellyfinId: "person-1",
              tmdbId: 31,
              name: "Tom Hanks",
              imageTag: null,
            },
            candidates: [],
            ambiguous: false,
          }),
          { status: 200 },
        )
      }
      expect(path).toContain("personId=person-1")
      return new Response(
        JSON.stringify({
          items: [
            item({ id: "m1", kind: "Movie", name: "Cast Away" }),
            item({ id: "s1", kind: "Series", name: "Band of Brothers", childCount: 1 }),
          ],
          total: 2,
        }),
        { status: 200 },
      )
    })
    vi.stubGlobal("fetch", fetchMock)

    renderSection("tom hanks")

    expect(await screen.findByText("Featuring Tom Hanks")).toBeTruthy()
    // Cards can print the title in more than one place (name + aria).
    expect(screen.getAllByText("Cast Away").length).toBeGreaterThan(0)
    expect(screen.getAllByText("Band of Brothers").length).toBeGreaterThan(0)
    const seeAll = screen.getByRole("link", { name: "See all" })
    expect(seeAll.getAttribute("href")).toContain("mode=person")
    expect(seeAll.getAttribute("href")).toContain("personId=person-1")
  })

  test("renders nothing when no exact person matches the term", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      expect(path).toContain("/api/person/resolve")
      return new Response(
        JSON.stringify({ person: null, candidates: [], ambiguous: false }),
        { status: 200 },
      )
    })
    vi.stubGlobal("fetch", fetchMock)

    const { container } = renderSection("severance")

    // Give the resolve query a tick to settle before asserting emptiness.
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled())
    expect(container.firstChild).toBeNull()
    // Without a person there is nothing to look up — the items query must not run.
    expect(
      fetchMock.mock.calls.filter(([path]) => String(path).includes("/api/items")),
    ).toHaveLength(0)
  })

  test("stays silent when the server cannot resolve people at all", async () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 503 }))
    vi.stubGlobal("fetch", fetchMock)

    const { container } = renderSection("tom hanks")

    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled())
    expect(container.firstChild).toBeNull()
  })
})
