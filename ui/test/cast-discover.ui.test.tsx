import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { CastDiscover } from "../src/components/seerr/CastDiscover"
import type { SeerrResult, SeerrStatusInfo, Status } from "../src/lib/api"
import { castDiscoverResults } from "../src/lib/cast-search"
import { queryKeys } from "../src/lib/query-client"

const linked: SeerrStatusInfo = {
  configured: true,
  linked: true,
  expired: false,
  serverUrl: "https://seerr.test",
  instance: {
    movie4kEnabled: false,
    series4kEnabled: false,
    partialRequestsEnabled: true,
  },
  user: { id: 1, name: "Neo", avatar: null, jellyfinUserId: "neo" },
  capabilities: null,
  quota: null,
}

const appStatus: Status = {
  authenticated: true,
  bootstrapped: true,
  bootstrap: { complete: true, ready: true, processed: 100, total: 100, initial: false },
  companion: undefined,
}

function result(patch: Partial<SeerrResult>): SeerrResult {
  return {
    mediaType: "movie",
    tmdbId: 1,
    title: "Title",
    year: 2020,
    overview: null,
    posterPath: null,
    backdropPath: null,
    voteAverage: null,
    status: "unknown",
    status4k: "unknown",
    libraryItemId: null,
    ...patch,
  }
}

function providers(client: QueryClient) {
  return function Providers({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={client}>
        <MemoryRouter>{children}</MemoryRouter>
      </QueryClientProvider>
    )
  }
}

function clientWithStatus(status: SeerrStatusInfo = linked, app: Status = appStatus) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(queryKeys.seerrStatus, status)
  client.setQueryData(queryKeys.status, app)
  return client
}

afterEach(() => vi.restoreAllMocks())

describe("cast Discover results", () => {
  test("keeps live person pages out of progressive local-query invalidations", () => {
    expect(queryKeys.items({ personId: "jf-keanu" })[0]).toBe("person-items")
    expect(queryKeys.items({ search: "Keanu Reeves" })[0]).toBe("items")
  })

  test("drops every locally available identity and provider duplicate without losing status", () => {
    const values = [
      result({ tmdbId: 603, title: "The Matrix" }),
      result({ tmdbId: 603, title: "The Matrix", libraryItemId: "m1", status: "available" }),
      result({ tmdbId: 245891, title: "John Wick", status: "pending" }),
      result({ tmdbId: 245891, title: "John Wick again", status: "pending" }),
      result({ mediaType: "tv", tmdbId: 603, title: "A series", status: "partial" }),
    ]

    expect(castDiscoverResults(values)).toEqual([values[2], values[4]])
  })

  test("reuses Seerr cards and request-state conventions for live-verified non-local titles", () => {
    const client = clientWithStatus()
    client.setQueryData(queryKeys.seerrPersonCredits(6384, "jf-keanu"), {
      page: 1,
      totalPages: 1,
      totalResults: 2,
      results: [
        result({ tmdbId: 603, title: "The Matrix", libraryItemId: "m1" }),
        result({ tmdbId: 245891, title: "John Wick", status: "pending" }),
      ],
    })

    render(
      <CastDiscover
        personName="Keanu Reeves"
        jellyfinId="jf-keanu"
        tmdbId={6384}
      />,
      { wrapper: providers(client) },
    )

    expect(screen.getByRole("heading", { name: "Discover" })).toBeTruthy()
    expect(screen.getAllByText("John Wick").length).toBeGreaterThan(0)
    expect(screen.getByText("Requested")).toBeTruthy()
    expect(screen.queryByText("The Matrix")).toBeNull()
  })

  test("a pending Seerr round trip never hides already rendered server results", () => {
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})))
    const client = clientWithStatus()

    render(
      <>
        <output>Server results ready</output>
        <CastDiscover
          personName="Keanu Reeves"
          jellyfinId="jf-keanu"
          tmdbId={6384}
        />
      </>,
      { wrapper: providers(client) },
    )

    expect(screen.getByText("Server results ready")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "Discover" })).toBeTruthy()
    expect(document.querySelectorAll(".h-poster-h").length).toBe(4)
  })

  test("waits for an incomplete progressive catalog when no exact Jellyfin identity exists", () => {
    const client = clientWithStatus(linked, {
      ...appStatus,
      bootstrapped: false,
      bootstrap: { complete: false, ready: true, processed: 200, total: 1000, initial: true },
    })
    render(
      <CastDiscover personName="Keanu Reeves" jellyfinId={null} tmdbId={6384} />,
      { wrapper: providers(client) },
    )

    expect(screen.getByText(/Finishing the progressive library catalog/)).toBeTruthy()
    expect(screen.queryByText("Request")).toBeNull()
  })

  test("an unconfigured Seerr explains the limitation without affecting local results", () => {
    const client = clientWithStatus({ ...linked, configured: false, linked: false })
    render(
      <CastDiscover
        personName="Keanu Reeves"
        jellyfinId="jf-keanu"
        tmdbId={6384}
      />,
      { wrapper: providers(client) },
    )

    expect(screen.getByText(/Seerr is not connected/)).toBeTruthy()
    expect(screen.getByRole("link", { name: "Open Seerr settings" })).toBeTruthy()
  })
})
