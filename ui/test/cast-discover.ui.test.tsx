import type { QueryClient } from "@tanstack/react-query"
import { act, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, describe, expect, test, vi } from "vitest"
import { CastDiscover } from "../src/components/seerr/CastDiscover"
import { api, type SeerrResult, type SeerrStatusInfo, type Status } from "../src/lib/api"
import { castDiscoverResults } from "../src/lib/cast-search"
import { queryKeys } from "../src/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { appStatus } from "./support/fixtures"
import { TestProviders } from "./test-utils"

const linked: SeerrStatusInfo = {
  linked: true,
  mapped: true,
  instance: {
    movie4kEnabled: false,
    series4kEnabled: false,
    partialRequestsEnabled: true,
  },
  user: { id: 1, name: "Neo", avatar: null, jellyfinUserId: "neo" },
  capabilities: null,
  quota: null,
}

const signedIn: Status = appStatus({
  authenticated: true,
  bootstrapped: true,
  bootstrap: { complete: true, ready: true, processed: 100, total: 100, initial: false },
})

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
      <TestProviders client={client}>{children}</TestProviders>
    )
  }
}

function clientWithStatus(status: SeerrStatusInfo = linked, app: Status = signedIn) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.seerrStatus, status)
  client.setQueryData(queryKeys.status, app)
  return client
}

afterEach(() => vi.restoreAllMocks())

describe("cast Discover results", () => {
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

  test("without an exact Jellyfin identity, withholds requestable titles until the catalog is complete", async () => {
    vi.spyOn(api.seerr, "personCredits").mockResolvedValue({
      page: 1,
      totalPages: 1,
      totalResults: 1,
      results: [result({ tmdbId: 245891, title: "John Wick" })],
    })
    const client = clientWithStatus(linked, {
      ...signedIn,
      bootstrapped: false,
      bootstrap: { complete: false, ready: true, processed: 200, total: 1000, initial: true },
    })
    render(
      <CastDiscover personName="Keanu Reeves" jellyfinId={null} tmdbId={6384} />,
      { wrapper: providers(client) },
    )

    await act(() => new Promise((resolve) => setTimeout(resolve, 0)))
    expect(screen.queryByText("John Wick")).toBeNull()

    act(() => client.setQueryData(queryKeys.status, signedIn))
    expect((await screen.findAllByText("John Wick")).length).toBeGreaterThan(0)
  })
})
