import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import {
  FranchiseCollections,
  JellyfinCollections,
  MyCollections,
} from "../src/routes/Collections"
import { PreviewProvider } from "../src/components/PreviewCard"
import { MyCollectionDetail } from "../src/routes/CollectionDetail"
import CollectionSettingsPage from "../src/routes/CollectionSettings"
import DiscoverDetail from "../src/routes/DiscoverDetail"
import type {
  ClassifiedCollectionTitle,
  CollectionProfile,
  ItemDetail,
  NormalizedCollectionTitle,
  SeerrMediaDetail,
  SeerrStatusInfo,
} from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryClient, queryKeys } from "../src/lib/query-client"

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

function libraryItem(id: string, name: string): ItemDetail {
  return {
    id,
    kind: "Movie",
    name,
    year: 1999,
    runtimeTicks: 8_160_000_000,
    communityRating: 8.7,
    officialRating: "R",
    seriesId: null,
    seriesName: null,
    indexNumber: null,
    parentIndexNumber: null,
    primaryImageTag: "poster",
    thumbImageTag: null,
    logoImageTag: null,
    backdropImageTag: null,
    childCount: null,
    premiereDate: "1999-03-31T00:00:00Z",
    seasonId: null,
    played: false,
    playCount: 0,
    positionTicks: 0,
    favorite: false,
    genres: ["Action"],
    originalTitle: null,
    providerIds: { tmdb: null, imdb: null, tvdb: null },
    parentId: null,
    dateCreated: null,
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
    template: { id: "tmdb.discover.movie.popular", version: 1 },
    title: name,
    description: "",
    customPosterId: null,
    source: { kind: "tmdbDiscover", schemaVersion: 1, parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    ordering: "source",
    cadence: "daily",
  }
}

function providers(
  ui: ReactNode,
  initialEntry: string,
  path = "*",
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  client.setQueryData(queryKeys.status, {
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-1",
    userName: "Neo",
  })
  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <Routes><Route path={path} element={ui} /></Routes>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

afterEach(() => {
  vi.restoreAllMocks()
  queryClient.clear()
})

describe("mode-aware collections", () => {
  test("collection settings renders the flat template contract and opens its wizard", async () => {
    vi.spyOn(api.api.collections, "settings").mockResolvedValue({
      effectiveMode: "mediaFlick",
      mediaFlickAvailable: true,
      modeSelection: "mediaFlick",
      franchises: { includeUnreleased: false },
      readiness: { tmdb: true, mdblist: false },
      recovery: null,
      access: { readOnly: false },
    })
    vi.spyOn(api.api.collections, "profiles").mockResolvedValue({ profiles: [] })
    vi.spyOn(api.api.collections, "templates").mockResolvedValue({
      categories: ["popular"],
      templates: [{
        template: {
          id: "tmdb.discover.movie.popular",
          version: 1,
          title: "Popular movies",
          description: "Popular movies from TMDB.",
          category: "popular",
          pictogram: "star",
          source: { kind: "tmdbDiscover", schemaVersion: 1, parameters: {} },
          mediaType: "movie",
          limit: { kind: "all" },
          ordering: "source",
          cadence: "daily",
        },
        available: true,
      }],
      readiness: { tmdb: true, mdblist: false },
    })

    render(providers(<CollectionSettingsPage />, "/settings/collections"))
    const template = await screen.findByRole("button", { name: /Popular movies/ })
    fireEvent.click(template)

    expect(await screen.findByRole("heading", { name: "Add collection" })).toBeTruthy()
    expect(screen.getByLabelText("Title")).toHaveProperty("value", "Popular movies")
  })

  test("franchise cards use exact TMDB collection identities", async () => {
    const detail = vi.spyOn(api.api.collections, "franchise").mockResolvedValue({
      collectionId: 2344,
      name: "The Matrix Collection",
      posterPath: null,
      backdropPath: null,
      owned: [],
      missing: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
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
    expect(document.querySelector('a[href="/collections/franchises/2344"]')).toBeTruthy()
    expect(screen.getByText("1 owned · 1 missing")).toBeTruthy()
    fireEvent.pointerEnter(link)
    await waitFor(() => expect(detail).toHaveBeenCalled())
    expect(detail.mock.calls[0]?.[0]).toBe(2344)
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
    const detail = vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: first,
      status: "ready",
      owned: [],
      missing: [],
      items: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
    vi.spyOn(api.api.collections, "mine").mockResolvedValue({
      profiles: [first, profile("c".repeat(16), "Second")],
    })
    render(providers(<MyCollections />, "/collections/mine"))
    const firstLink = await screen.findByRole("link", { name: "Open First" })
    const names = screen.getAllByRole("link", { name: /Open (First|Second)/ }).map((link) => link.getAttribute("aria-label"))
    expect(names).toEqual(["Open First", "Open Second"])
    fireEvent.pointerEnter(firstLink)
    await waitFor(() => expect(detail).toHaveBeenCalledWith(first.id, expect.any(AbortSignal)))
  })

  test("Jellyfin mode loads BoxSets directly", async () => {
    const jellyfin = vi.spyOn(api.api.collections, "jellyfin").mockResolvedValue({
      collections: [{ id: "box-1", name: "Holiday films", primaryImageTag: null, backdropImageTag: null, itemCount: 4 }],
    })
    render(providers(<JellyfinCollections />, "/collections/jellyfin"))
    expect(await screen.findByRole("link", { name: "Open Holiday films" })).toBeTruthy()
    expect(jellyfin).toHaveBeenCalled()
    expect(document.querySelector('a[href="/collections/jellyfin/box-1"]')).toBeTruthy()
  })

  test("Missing stays expanded at 24 and collapses at 25", async () => {
    const id = "d".repeat(16)
    const current = profile(id, "Popular movies")
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: current,
      status: "ready",
      owned: [owned(1)],
      missing: Array.from({ length: 25 }, (_, index) => title(index + 10)),
      items: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
    render(providers(<MyCollectionDetail />, `/collections/mine/${id}`, "/collections/mine/:profileId"))
    const expand = await screen.findByRole("button", { name: "Show all 25" })
    expect(screen.queryAllByText("Movie 34")).toHaveLength(0)
    fireEvent.click(expand)
    expect(screen.getAllByText("Movie 34").length).toBeGreaterThan(0)
    expect(screen.getByRole("button", { name: "Show fewer" })).toBeTruthy()
  })

  test("Missing stays fully expanded at 24", async () => {
    const id = "1".repeat(16)
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: profile(id, "Twenty four"),
      status: "ready",
      owned: [],
      missing: Array.from({ length: 24 }, (_, index) => title(index + 10)),
      items: [],
      libraryItems: [],
      ownershipAvailable: true,
    })
    render(providers(<MyCollectionDetail />, `/collections/mine/${id}`, "/collections/mine/:profileId"))
    expect(await screen.findAllByText("Movie 33")).not.toHaveLength(0)
    expect(screen.queryByRole("button", { name: /Show all/ })).toBeNull()
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
    const chooser = await screen.findByText("Choose edition")
    expect(chooser.closest("details")?.querySelectorAll('a[href^="/item/"]')).toHaveLength(2)
  })

  test("collection contents use the standard library and discovery card controls", async () => {
    const id = "2".repeat(16)
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue({
      profile: profile(id, "Card controls"),
      status: "ready",
      owned: [owned(1)],
      missing: [title(2)],
      items: [],
      libraryItems: [libraryItem("local-1-0", "Movie 1")],
      ownershipAvailable: true,
    })
    const itemRequest = vi.spyOn(api.api, "item")
    vi.spyOn(api.api.seerr, "status").mockResolvedValue(seerrStatus)

    render(providers(
      <PreviewProvider enabled={false}>
        <MyCollectionDetail />
      </PreviewProvider>,
      `/collections/mine/${id}`,
      "/collections/mine/:profileId",
    ))

    expect(await screen.findByRole("button", { name: "Play" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Add to My List" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Mark as watched" })).toBeTruthy()
    expect(await screen.findByRole("button", { name: "Request Movie 2" })).toBeTruthy()
    expect(document.querySelector('a[href="/discover/movie/2"]')).toBeTruthy()
    expect(itemRequest).not.toHaveBeenCalled()
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
    const media = vi.spyOn(api.api.seerr, "media")
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

    expect(await screen.findByText("Downloading")).toBeTruthy()
    expect(media).toHaveBeenCalledTimes(2)
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
    expect(screen.getByText("Seerr is unavailable for this title. Request actions are not available.")).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Request/i })).toBeNull()
  })
})
