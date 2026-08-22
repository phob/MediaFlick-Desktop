import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import Collections from "../src/routes/Collections"
import CollectionDetail from "../src/routes/CollectionDetail"
import type {
  BoxSetDetail,
  CollectionDetail as CollectionDetailData,
  ItemDetail as LocalItemDetail,
  ItemSummary,
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

function localItem(patch: Partial<LocalItemDetail> = {}): LocalItemDetail {
  return {
    id: "m1",
    kind: "Movie",
    name: "The Matrix",
    year: 1999,
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
    genres: [],
    originalTitle: null,
    providerIds: { tmdb: "603", imdb: "tt0133093", tvdb: null },
    parentId: null,
    dateCreated: null,
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

  test("loads every pending collection batch before rendering the index", async () => {
    const index = vi.spyOn(api.api.collections, "index")
      .mockResolvedValueOnce({
        collections: [collectionSummary({ id: 10, name: "The Matrix Collection" })],
        libraryMovies: 5,
        mappedMovies: 2,
        pendingMovies: 2,
      })
      .mockResolvedValueOnce({
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
      expect(screen.getByText("Alien Collection")).toBeTruthy()
    })
    expect(index).toHaveBeenCalledTimes(2)
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
      expect(screen.getByText("No collections found")).toBeTruthy()
    })
  })

  test("renders native BoxSets with links to their own pages", async () => {
    vi.spyOn(api.api.collections, "index").mockResolvedValue({
      source: "jellyfin",
      collections: [
        collectionSummary({
          id: "bs-1",
          name: "The Matrix Collection",
          primaryImageTag: "tag-1",
          movieCount: 2,
        }),
        collectionSummary({ id: "bs-2", name: "Christmas films", movieCount: null }),
      ],
    })
    render(providers(<Collections />, "/collections"))
    await waitFor(() => {
      expect(screen.getByText("The Matrix Collection")).toBeTruthy()
    })
    expect(screen.getByText("Christmas films")).toBeTruthy()
    expect(document.querySelector('a[href="/collections/bs-1"]')).toBeTruthy()
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
    const item = vi.spyOn(api.api, "item").mockResolvedValue(localItem())
    render(providers(<CollectionDetail />, "/collections/10"))
    await waitFor(() => {
      expect(screen.getByText("1 of 2 in your library · 1 missing")).toBeTruthy()
    })
    const localLink = await screen.findByRole("link", { name: "Open details for The Matrix" })
    expect(localLink.getAttribute("href")).toBe("/item/m1")
    expect(item).toHaveBeenCalledWith("m1")
    expect(document.querySelector('a[href="/discover/movie/624834"]')).toBeTruthy()
    expect(screen.getAllByText("The Matrix Resurrections").length).toBeGreaterThan(0)
  })

  test("native BoxSet detail renders server children and Seerr-known missing parts", async () => {
    const boxset: BoxSetDetail = {
      id: "bs-1",
      tmdbId: 10,
      name: "The Matrix Collection",
      primaryImageTag: null,
      backdropImageTag: null,
      items: [itemSummary({ id: "m1", name: "The Matrix" })],
    }
    vi.spyOn(api.api.collections, "boxset").mockResolvedValue(boxset)
    const detail: CollectionDetailData = {
      id: 10,
      name: "The Matrix Collection",
      overview: null,
      posterPath: null,
      backdropPath: null,
      parts: [
        result({ tmdbId: 603, title: "The Matrix", libraryItemId: "m1" }),
        result({ tmdbId: 624834, title: "The Matrix Resurrections" }),
      ],
    }
    vi.spyOn(api.api.collections, "detail").mockResolvedValue(detail)
    vi.spyOn(api.api, "item").mockResolvedValue(localItem())
    render(providers(<CollectionDetail />, "/collections/bs-1"))

    await waitFor(() => {
      expect(screen.getByText("1 of 2 in your library · 1 missing")).toBeTruthy()
    })
    const localLink = await screen.findByRole("link", { name: "Open details for The Matrix" })
    expect(localLink.getAttribute("href")).toBe("/item/m1")
    expect(document.querySelector('a[href="/discover/movie/624834"]')).toBeTruthy()
  })

  test("native BoxSet detail without a TMDB identity shows only its children", async () => {
    const boxset: BoxSetDetail = {
      id: "bs-9",
      tmdbId: null,
      name: "Christmas films",
      primaryImageTag: null,
      backdropImageTag: null,
      items: [
        itemSummary({ id: "m1", name: "Die Hard" }),
        itemSummary({ id: "m2", name: "Home Alone" }),
      ],
    }
    vi.spyOn(api.api.collections, "boxset").mockResolvedValue(boxset)
    const detail = vi.spyOn(api.api.collections, "detail")
    render(providers(<CollectionDetail />, "/collections/bs-9"))

    await waitFor(() => {
      expect(screen.getByText("2 movies")).toBeTruthy()
    })
    expect(detail).not.toHaveBeenCalled()
    expect(screen.queryByText("Missing from your library")).toBeNull()
  })
})

function itemSummary(patch: Partial<ItemSummary> & Pick<ItemSummary, "id" | "name">): ItemSummary {
  return {
    kind: "Movie",
    year: 1999,
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
    ...patch,
  }
}
