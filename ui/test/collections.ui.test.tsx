import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import Collections from "../src/routes/Collections"
import CollectionDetail from "../src/routes/CollectionDetail"
import type {
  CollectionDetail as CollectionDetailData,
  SeerrResult,
  SeerrStatusInfo,
} from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"

const linkedSeerr: SeerrStatusInfo = {
  configured: true,
  linked: true,
  expired: false,
  serverUrl: null,
  instance: { movie4kEnabled: false, series4kEnabled: false, partialRequestsEnabled: true },
  user: null,
  capabilities: null,
  quota: null,
}

function collectionSummary(
  patch: Partial<api.CollectionSummary>,
): api.CollectionSummary {
  return {
    id: 10,
    name: "The Matrix Collection",
    posterPath: null,
    backdropPath: null,
    movieCount: 2,
    ...patch,
  }
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

function providers(ui: ReactNode, initialEntry: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(queryKeys.seerrStatus, linkedSeerr)
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <Routes>
          <Route path="/collections" element={ui} />
          <Route path="/collections/:id" element={ui} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

afterEach(() => vi.restoreAllMocks())

describe("Collections", () => {
  test("renders one card per derived collection with its movie count", async () => {
    vi.spyOn(api.api.collections, "index").mockResolvedValue({
      collections: [
        collectionSummary({ id: 10, name: "The Matrix Collection" }),
        collectionSummary({ id: 2, name: "Alien Collection", movieCount: 1 }),
      ],
      libraryMovies: 5,
      mappedMovies: 3,
      pendingMovies: 0,
    })
    render(providers(<Collections />, "/collections"))
    await waitFor(() => {
      expect(screen.getByText("The Matrix Collection")).toBeTruthy()
    })
    expect(screen.getByText("Alien Collection")).toBeTruthy()
    expect(screen.getByText("2 movies")).toBeTruthy()
    expect(screen.getByText("1 movie")).toBeTruthy()
  })

  test("explains an empty index instead of showing a broken grid", async () => {
    vi.spyOn(api.api.collections, "index").mockResolvedValue({
      collections: [],
      libraryMovies: 0,
      mappedMovies: 0,
      pendingMovies: 0,
    })
    render(providers(<Collections />, "/collections"))
    await waitFor(() => {
      expect(screen.getByText("No collections yet")).toBeTruthy()
    })
  })

  test("collection detail counts owned parts against missing ones", async () => {
    const detail: CollectionDetailData = {
      id: 10,
      name: "The Matrix Collection",
      overview: "Enter the Matrix.",
      posterPath: null,
      backdropPath: "/backdrop.jpg",
      parts: [
        result({ tmdbId: 603, title: "The Matrix", libraryItemId: "m1" }),
        result({ tmdbId: 624834, title: "The Matrix Resurrections" }),
      ],
    }
    vi.spyOn(api.api.collections, "detail").mockResolvedValue(detail)
    render(providers(<CollectionDetail />, "/collections/10"))
    await waitFor(() => {
      expect(screen.getByText("1 of 2 in your library · 1 missing")).toBeTruthy()
    })
    // Both halves of the surface exist: the owned title and the discoverable
    // one that carries the request flow. Each title renders in a poster
    // placeholder and its caption, hence the multiples.
    expect(screen.getAllByText("The Matrix Resurrections").length).toBeGreaterThan(0)
    expect(screen.getAllByText("The Matrix").length).toBeGreaterThan(0)
  })
})
