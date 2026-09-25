import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import {
  FranchiseCollections,
  JellyfinCollections,
  MyCollections,
} from "../src/routes/Collections"
import { MyCollectionDetail } from "../src/routes/CollectionDetail"
import DiscoverDetail from "../src/routes/DiscoverDetail"
import type {
  ClassifiedCollectionTitle,
  CollectionProfile,
  NormalizedCollectionTitle,
  SeerrMediaDetail,
  SeerrStatusInfo,
} from "../src/lib/api"
import * as api from "../src/lib/api"
import { createQueryClient, queryKeys } from "../src/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const queryClient = createQueryClient()

function title(id: number, patch: Partial<NormalizedCollectionTitle> = {}): NormalizedCollectionTitle {
  return {
    mediaType: "movie",
    tmdbId: id,
    title: `Movie ${id}`,
    overview: "",
    sourceOrder: id,
    adult: false,
    ...patch,
  }
}

function owned(id: number, editions = 1): ClassifiedCollectionTitle {
  return {
    ...title(id),
    localItems: Array.from({ length: editions }, (_, index) => ({
      id: `local-${id}-${index}`,
      name: index === 0 ? `Movie ${id}` : `Movie ${id} edition ${index + 1}`,
      kind: "Movie",
      played: false,
    })),
  }
}

const seerrStatus: SeerrStatusInfo = {
  linked: true,
  mapped: true,
  instance: {
    movie4kEnabled: false,
    series4kEnabled: false,
    partialRequestsEnabled: true,
  },
  user: { id: 1, name: "Neo", avatar: null, jellyfinUserId: "user-1" },
  capabilities: {
    movie: { request: true, autoApprove: false },
    tv: { request: true, autoApprove: false },
    movie4k: { request: false, autoApprove: false },
    tv4k: { request: false, autoApprove: false },
    advancedRequest: false,
  },
  quota: {
    movie: { days: null, limit: null, used: 0, remaining: null, restricted: false },
    tv: { days: null, limit: null, used: 0, remaining: null, restricted: false },
  },
}

function seerrMedia(id: number, status: SeerrMediaDetail["status"]): SeerrMediaDetail {
  return {
    mediaType: "movie",
    tmdbId: id,
    title: `Movie ${id}`,
    year: 1999,
    overview: "",
    posterPath: null,
    backdropPath: null,
    voteAverage: null,
    status,
    status4k: "unknown",
    libraryItemId: null,
    runtimeMinutes: null,
    genres: [],
    seasons: [],
    tagline: null,
    originalTitle: null,
    voteCount: null,
    releaseDate: null,
    firstAirDate: null,
    lastAirDate: null,
    productionStatus: null,
    inProduction: false,
    seriesType: null,
    numberOfSeasons: null,
    numberOfEpisodes: null,
    originalLanguage: null,
    homepage: null,
    externalIds: { imdb: null, tvdb: null },
    budget: null,
    revenue: null,
    studios: [],
    networks: [],
    creators: [],
    directors: [],
    writers: [],
    productionCountries: [],
    spokenLanguages: [],
    cast: [],
    trailer: null,
    releaseDates: [],
    contentRatings: [],
    nextEpisode: null,
  }
}

function profile(id: string, name: string): CollectionProfile {
  return {
    id,
    revision: "b".repeat(16),
    template: { id: "tmdb.discover.movie.popular" },
    title: name,
    description: "",
    customPosterId: null,
    source: { kind: "tmdbDiscover", parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    cadence: "daily",
    availableOnHome: false,
  }
}

function providers(
  ui: ReactNode,
  initialEntry: string,
  path = "*",
  client = testQueryClient(),
) {
  client.setQueryData(queryKeys.status, {
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-1",
    userName: "Neo",
  })
  return (
    <TestProviders client={client} initialEntries={[initialEntry]}>
        <Routes><Route path={path} element={ui} /></Routes>
    </TestProviders>
  )
}

afterEach(() => {
  vi.restoreAllMocks()
  queryClient.clear()
})

describe("mode-aware collections", () => {
  test("franchise cards use exact TMDB collection identities", async () => {
    vi.spyOn(api.api.collections, "franchises").mockResolvedValue({
      status: "ready",
      franchises: [{
        collectionId: 2344,
        name: "The Matrix Collection",
        posterPath: null,
        backdropPath: null,
        ownedCount: 1,
        missingCount: 1,
        ownershipAvailable: true,
      }],
    })
    render(providers(<FranchiseCollections />, "/collections/franchises"))
    const link = await screen.findByRole("link", { name: "Open The Matrix Collection" })
    expect(link.getAttribute("href")).toBe("/collections/franchises/2344")
  })

  test("an uninitialized franchise cache stays in the background rebuilding state", async () => {
    vi.spyOn(api.api.collections, "franchises").mockResolvedValue({
      status: "updating",
      franchises: [],
    })
    render(providers(<FranchiseCollections />, "/collections/franchises"))
    expect(await screen.findByText("Finding movie franchises...")).toBeTruthy()
    expect(screen.queryByText("No movie franchises found.")).toBeNull()
  })

  test("My Collections preserves profile order", async () => {
    const first = profile("a".repeat(16), "First")
    vi.spyOn(api.api.collections, "mine").mockResolvedValue({
      profiles: [first, profile("c".repeat(16), "Second")],
    })
    render(providers(<MyCollections />, "/collections/mine"))
    await screen.findByRole("link", { name: "Open First" })
    const names = screen.getAllByRole("link", { name: /Open (First|Second)/ }).map((link) => link.getAttribute("aria-label"))
    expect(names).toEqual(["Open First", "Open Second"])
  })

  test("Jellyfin mode loads BoxSets directly", async () => {
    vi.spyOn(api.api.collections, "jellyfin").mockResolvedValue({
      collections: [{ id: "box-1", name: "Holiday films", primaryImageTag: null, backdropImageTag: null, itemCount: 4 }],
    })
    render(providers(<JellyfinCollections />, "/collections/jellyfin"))
    const link = await screen.findByRole("link", { name: "Open Holiday films" })
    expect(link.getAttribute("href")).toBe("/collections/jellyfin/box-1")
  })

  test("multiple local editions remain one Owned card with a chooser", async () => {
    const id = "e".repeat(16)
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: profile(id, "Editions"),
      status: "ready",
      owned: [owned(1, 2)],
      missing: [],
      items: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
    render(providers(<MyCollectionDetail />, `/collections/mine/${id}`, "/collections/mine/:profileId"))
    const secondEdition = await screen.findByRole("link", { name: "Movie 1 edition 2" })
    expect(secondEdition.getAttribute("href")).toBe("/item/local-1-1")
    expect(screen.getAllByRole("link", { name: "Movie 1" }).map((link) => link.getAttribute("href")))
      .toContain("/item/local-1-0")
  })

  test("a missing collection title links to its discovery page", async () => {
    const id = "2".repeat(16)
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: profile(id, "Card controls"),
      status: "ready",
      owned: [],
      missing: [title(2)],
      items: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
    vi.spyOn(api.api.seerr, "status").mockResolvedValue(seerrStatus)

    render(providers(<MyCollectionDetail />, `/collections/mine/${id}`, "/collections/mine/:profileId"))

    expect(await screen.findByRole("button", { name: "Request Movie 2" })).toBeTruthy()
    expect(document.querySelector('a[href="/discover/movie/2"]')).toBeTruthy()
  })

  test("a requested collection card refreshes to its current Seerr status", async () => {
    const id = "3".repeat(16)
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: profile(id, "Request status"),
      status: "ready",
      owned: [],
      missing: [title(2)],
      items: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
    vi.spyOn(api.api.seerr, "status").mockResolvedValue(seerrStatus)
    vi.spyOn(api.api.seerr, "media")
      .mockResolvedValueOnce(seerrMedia(2, "unknown"))
      .mockResolvedValue(seerrMedia(2, "processing"))
    vi.spyOn(api.api.seerr, "request").mockResolvedValue({
      id: 12,
      status: "approved",
      mediaType: "movie",
      tmdbId: 2,
      is4k: false,
      createdAt: null,
      updatedAt: null,
      mediaStatus: "processing",
      seasons: [],
      libraryItemId: null,
    })

    render(providers(
      <MyCollectionDetail />,
      `/collections/mine/${id}`,
      "/collections/mine/:profileId",
      queryClient,
    ))

    fireEvent.click(await screen.findByRole("button", { name: "Request Movie 2" }))
    fireEvent.click(screen.getByRole("button", { name: "Request" }))

    expect(await screen.findByText("Processing")).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Request Movie 2" })).toBeNull()
  })

  test("an untrusted library sync shows the ungrouped snapshot without request links", async () => {
    const id = "f".repeat(16)
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: profile(id, "Offline snapshot"),
      status: "ready",
      owned: [],
      missing: [],
      items: [title(10), title(11)],
      libraryItems: [],
      ownershipAvailable: false,
    })
    render(providers(<MyCollectionDetail />, `/collections/mine/${id}`, "/collections/mine/:profileId"))
    expect(await screen.findByText("Ownership unavailable")).toBeTruthy()
    await waitFor(() => expect(screen.getAllByText("Movie 10").length).toBeGreaterThan(0))
    expect(document.querySelector('a[href="/discover/movie/10"]')).toBeNull()
  })

  test("a collection title remains readable without Seerr and has no request action", async () => {
    vi.spyOn(api.api.seerr, "media").mockRejectedValue(new Error("offline"))
    vi.spyOn(api.api.seerr, "status").mockRejectedValue(new Error("offline"))
    vi.spyOn(api.api.collections, "title").mockResolvedValue({
      item: title(603, { title: "The Matrix", overview: "A simulated world.", year: 1999 }),
    })
    render(providers(<DiscoverDetail />, "/discover/movie/603", "/discover/:mediaType/:tmdbId"))
    expect(await screen.findByRole("heading", { name: "The Matrix" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Request/i })).toBeNull()
  })
})
