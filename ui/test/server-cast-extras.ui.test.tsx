import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { ServerCastExtras } from "../src/components/seerr/ServerCastExtras"
import type { ItemSummary, SeerrStatusInfo, Status } from "../src/lib/api"
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

function summary(id: string, name: string): ItemSummary {
  return {
    id,
    kind: "Movie",
    name,
    year: 1990,
    runtimeTicks: null,
    communityRating: 8.4,
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
  }
}

function renderExtras(client: QueryClient, ui: ReactNode) {
  function Providers({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={client}>
        <MemoryRouter>{children}</MemoryRouter>
      </QueryClientProvider>
    )
  }
  render(ui, { wrapper: Providers })
}

describe("proven server cast extras", () => {
  test("renders only backend-proven titles as library cards", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    client.setQueryData(queryKeys.seerrStatus, linked)
    client.setQueryData(queryKeys.status, appStatus)
    client.setQueryData(queryKeys.seerrPersonCredits(6384, "jf-slj"), {
      page: 1,
      totalPages: 1,
      totalResults: 1,
      results: [],
      libraryExtras: [summary("good1", "GoodFellas")],
    })

    renderExtras(
      client,
      <ServerCastExtras personName="Samuel L Jackson" jellyfinId="jf-slj" tmdbId={6384} />,
    )

    expect(screen.getByRole("heading", { name: "More on your Jellyfin server" })).toBeTruthy()
    expect(screen.getAllByText("GoodFellas").length).toBeGreaterThan(0)
  })

  test("stays silent until the backend has proven at least one title", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    client.setQueryData(queryKeys.seerrStatus, linked)
    client.setQueryData(queryKeys.status, appStatus)
    client.setQueryData(queryKeys.seerrPersonCredits(6384, "jf-slj"), {
      page: 1,
      totalPages: 1,
      totalResults: 0,
      results: [],
    })

    renderExtras(
      client,
      <>
        <output data-testid="anchor" />
        <ServerCastExtras personName="Samuel L Jackson" jellyfinId="jf-slj" tmdbId={6384} />
      </>,
    )

    expect(screen.getByTestId("anchor")).toBeTruthy()
    expect(screen.queryByText("More on your Jellyfin server")).toBeNull()
  })

  test("an unlinked Seerr never renders the section", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    client.setQueryData(queryKeys.seerrStatus, { ...linked, configured: false, linked: false })
    client.setQueryData(queryKeys.status, appStatus)

    renderExtras(
      client,
      <>
        <output data-testid="anchor" />
        <ServerCastExtras personName="Samuel L Jackson" jellyfinId="jf-slj" tmdbId={6384} />
      </>,
    )

    expect(screen.queryByText("More on your Jellyfin server")).toBeNull()
  })
})
