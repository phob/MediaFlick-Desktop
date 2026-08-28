import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import CollectionSettingsPage from "../src/routes/CollectionSettings"
import type {
  CollectionProfile,
  CollectionSettings,
  CollectionTemplate,
  CollectionTemplates,
  NormalizedCollectionTitle,
} from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"

const accountSettings: CollectionSettings = {
  effectiveMode: "mediaFlick",
  mediaFlickAvailable: true,
  modeSelection: "mediaFlick",
  franchises: { includeUnreleased: false },
  readiness: { tmdb: true, mdblist: true },
  recovery: null,
  access: { readOnly: false },
}

function template(patch: Partial<CollectionTemplate> = {}): CollectionTemplate {
  return {
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
    ...patch,
  }
}

function profile(id: string, patch: Partial<CollectionProfile> = {}): CollectionProfile {
  return {
    id,
    revision: "b".repeat(16),
    template: { id: "tmdb.discover.movie.popular", version: 1 },
    title: "Popular movies",
    description: "Popular movies from TMDB.",
    customPosterId: null,
    source: { kind: "tmdbDiscover", schemaVersion: 1, parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    ordering: "source",
    cadence: "daily",
    ...patch,
  }
}

function title(id: number, patch: Partial<NormalizedCollectionTitle> = {}): NormalizedCollectionTitle {
  return {
    mediaType: "movie",
    tmdbId: id,
    title: `Movie ${id}`,
    overview: "",
    sourceOrder: id,
    posterPath: null,
    backdropPath: null,
    adult: false,
    ...patch,
  }
}

function preview(items = [title(603, {
  title: "The Matrix",
  year: 1999,
  posterPath: "/matrix.jpg",
})]) {
  return {
    items,
    total: items.length,
    movies: items.filter((item) => item.mediaType === "movie").length,
    series: items.filter((item) => item.mediaType === "series").length,
  }
}

function catalog(...templates: CollectionTemplate[]): CollectionTemplates {
  return {
    categories: [...new Set(templates.map((item) => item.category))],
    templates: templates.map((item) => ({ template: item, available: true })),
    readiness: { tmdb: true, mdblist: true },
  }
}

function mockPage(options: {
  settings?: CollectionSettings
  profiles?: CollectionProfile[]
  templates?: CollectionTemplates
} = {}) {
  vi.spyOn(api.api.collections, "settings").mockResolvedValue(options.settings ?? accountSettings)
  vi.spyOn(api.api.collections, "profiles").mockResolvedValue({ profiles: options.profiles ?? [] })
  vi.spyOn(api.api.collections, "templates").mockResolvedValue(
    options.templates ?? catalog(template()),
  )
}

function page(
  initialEntry = "/settings/collections",
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  client.setQueryData(queryKeys.status, {
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-1",
    userName: "Neo",
  })
  const providers = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <Routes><Route path="*" element={children} /></Routes>
      </MemoryRouter>
    </QueryClientProvider>
  )
  return { client, ...render(<CollectionSettingsPage />, { wrapper: providers }) }
}

async function openTemplate(name = "Popular movies") {
  fireEvent.click(await screen.findByRole("button", { name: new RegExp(`^${name}`) }))
  await screen.findByRole("heading", { name: "Add collection" })
}

function createButton() {
  return screen.getByRole("button", { name: "Create" })
}

function isDisabled(element: Element) {
  return element.hasAttribute("disabled")
}

afterEach(() => vi.restoreAllMocks())

describe("collection settings wizard", () => {
  test("offers only media types the selected provider implements", async () => {
    mockPage({
      templates: catalog(
        template(),
        template({
          id: "mdblist.public-list",
          title: "MDBList list",
          source: { kind: "mdbListPublicList", schemaVersion: 1, listId: "42" },
          mediaType: "mixed",
        }),
      ),
    })
    page()

    await openTemplate()
    fireEvent.click(screen.getByRole("combobox", { name: "Media type" }))
    expect(screen.queryByRole("option", { name: "Mixed" })).toBeNull()
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" })

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    await openTemplate("MDBList list")
    fireEvent.click(screen.getByRole("combobox", { name: "Media type" }))
    expect(screen.getByRole("option", { name: "Movie" })).toBeTruthy()
    expect(screen.getByRole("option", { name: "Series" })).toBeTruthy()
    expect(screen.getByRole("option", { name: "Mixed" })).toBeTruthy()
  })

  test("renders a colored Lucide pictogram instead of the shared template banner", async () => {
    mockPage()
    page()

    const card = await screen.findByRole("button", { name: /Popular movies/ })
    expect(card.querySelector(".lucide-star")).toBeTruthy()
    expect(card.querySelector("img")).toBeNull()
  })

  test("renders provider posters and commits only after a successful Preview", async () => {
    mockPage()
    const result = preview()
    const runPreview = vi.spyOn(api.api.collections, "preview").mockResolvedValue(result)
    const create = vi.spyOn(api.api.collections, "createProfile").mockResolvedValue({
      profile: profile("a".repeat(16)),
      total: 1,
    })
    page()
    await openTemplate()

    expect(isDisabled(createButton())).toBe(true)
    fireEvent.click(screen.getByRole("button", { name: "Preview" }))

    expect(await screen.findByText("1 total · 1 movies · 0 series")).toBeTruthy()
    expect(screen.getByText("The Matrix (1999)")).toBeTruthy()
    expect(document.querySelector('img[src="/api/collections/provider-artwork?path=%2Fmatrix.jpg&size=w342"]')).toBeTruthy()
    expect(isDisabled(createButton())).toBe(false)

    fireEvent.click(createButton())
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1))
    expect(runPreview).toHaveBeenCalledWith(expect.any(Object), expect.any(AbortSignal))
  })

  test("shows at most 24 sampled titles while preserving the provider's full counts", async () => {
    mockPage()
    const items = Array.from({ length: 24 }, (_, index) => title(index + 1, {
      posterPath: `/poster-${index + 1}.jpg`,
    }))
    vi.spyOn(api.api.collections, "preview").mockResolvedValue({
      items,
      total: 87,
      movies: 60,
      series: 27,
    })
    page()
    await openTemplate()

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))

    expect(await screen.findByText("87 total · 60 movies · 27 series")).toBeTruthy()
    expect(screen.getAllByRole("listitem")).toHaveLength(24)
    expect(screen.getByText("Movie 24")).toBeTruthy()
  })

  test("rejects out-of-range maximums before calling Preview", async () => {
    const popular = template({ limit: { kind: "maximum", count: 20 } })
    mockPage({ templates: catalog(popular) })
    const runPreview = vi.spyOn(api.api.collections, "preview")
    page()
    await openTemplate()
    const maximum = screen.getByRole("spinbutton", { name: "Maximum results" })

    for (const value of ["0", "501", "1.5"]) {
      fireEvent.change(maximum, { target: { value } })
      expect(isDisabled(screen.getByRole("button", { name: "Preview" }))).toBe(true)
    }
    expect(runPreview).not.toHaveBeenCalled()
  })

  test("discards a Preview response that completes after a result change", async () => {
    const popular = template({ limit: { kind: "maximum", count: 20 } })
    mockPage({ templates: catalog(popular) })
    let resolvePreview!: (value: ReturnType<typeof preview>) => void
    const pending = new Promise<ReturnType<typeof preview>>((resolve) => {
      resolvePreview = resolve
    })
    const runPreview = vi.spyOn(api.api.collections, "preview").mockReturnValue(pending)
    page()
    await openTemplate()

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    fireEvent.change(screen.getByRole("spinbutton", { name: "Maximum results" }), {
      target: { value: "21" },
    })

    const signal = runPreview.mock.calls[0]?.[1]
    expect(signal?.aborted).toBe(true)
    await act(async () => {
      resolvePreview(preview())
      await pending
    })
    expect(screen.queryByText("The Matrix (1999)")).toBeNull()
    expect(isDisabled(createButton())).toBe(true)
    expect(screen.getByRole("button", { name: "Preview" })).toBeTruthy()
  })

  test("a failed repeat Preview clears the old result and blocks Create", async () => {
    mockPage()
    vi.spyOn(api.api.collections, "preview")
      .mockResolvedValueOnce(preview())
      .mockRejectedValueOnce(new Error("TMDB unavailable"))
    page()
    await openTemplate()

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    await screen.findByText("The Matrix (1999)")
    fireEvent.click(screen.getByRole("button", { name: "Preview again" }))

    expect((await screen.findByRole("alert")).textContent).toContain("Preview failed: TMDB unavailable")
    expect(screen.queryByText("The Matrix (1999)")).toBeNull()
    expect(isDisabled(createButton())).toBe(true)
  })

  test("source parameters, media type, and result limit each invalidate Preview", async () => {
    const custom = template({
      id: "tmdb.discover.movie.custom-discover",
      title: "Custom discover",
      source: { kind: "tmdbDiscover", schemaVersion: 1, parameters: {} },
      limit: { kind: "maximum", count: 20 },
    })
    mockPage({ templates: catalog(custom) })
    vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    page()
    await openTemplate("Custom discover")

    const previewAndExpectCreate = async () => {
      fireEvent.click(screen.getByRole("button", { name: "Preview" }))
      await screen.findByText("The Matrix (1999)")
      expect(isDisabled(createButton())).toBe(false)
    }

    await previewAndExpectCreate()
    fireEvent.change(screen.getByLabelText("Metadata language (optional)"), {
      target: { value: "de-DE" },
    })
    expect(isDisabled(createButton())).toBe(true)

    await previewAndExpectCreate()
    fireEvent.change(screen.getByRole("spinbutton", { name: "Maximum results" }), {
      target: { value: "25" },
    })
    expect(isDisabled(createButton())).toBe(true)

    await previewAndExpectCreate()
    fireEvent.click(screen.getByRole("combobox", { name: "Media type" }))
    fireEvent.click(await screen.findByRole("option", { name: "Series" }))
    expect(isDisabled(createButton())).toBe(true)
  })

  test("exact-collection release filtering invalidates Preview", async () => {
    const exact = template({
      id: "tmdb.collection.matrix",
      title: "The Matrix Collection",
      source: {
        kind: "tmdbCollection",
        schemaVersion: 1,
        collectionId: 2344,
        includeUnreleased: false,
      },
    })
    mockPage({ templates: catalog(exact) })
    vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    page()
    await openTemplate("The Matrix Collection")

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    await screen.findByText("The Matrix (1999)")
    fireEvent.click(document.querySelector("#collection-unreleased")!)

    expect(isDisabled(createButton())).toBe(true)
    expect(screen.queryByText("The Matrix (1999)")).toBeNull()
  })

  test("an MDBList selector change invalidates Preview", async () => {
    const list = template({
      id: "mdblist.public.custom",
      title: "Public list",
      source: { kind: "mdbListPublicList", schemaVersion: 1, listId: "42" },
      mediaType: "mixed",
    })
    mockPage({ templates: catalog(list) })
    vi.spyOn(api.api.collections, "preview").mockResolvedValue({
      ...preview(),
      sourceIdentity: "42",
    })
    page()
    await openTemplate("Public list")

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    await screen.findByText("The Matrix (1999)")
    fireEvent.change(screen.getByLabelText("MDBList public list ID or canonical URL"), {
      target: { value: "alice/favorites" },
    })

    expect(isDisabled(createButton())).toBe(true)
    expect(screen.queryByText("The Matrix (1999)")).toBeNull()
  })

  test("presentation-only edits save without Preview while the provider is unavailable", async () => {
    const current = profile("a".repeat(16))
    mockPage({
      settings: {
        ...accountSettings,
        readiness: { tmdb: false, mdblist: false },
      },
      profiles: [current],
    })
    const update = vi.spyOn(api.api.collections, "updateProfile").mockResolvedValue(current)
    page(`/settings/collections?edit=${current.id}`)

    await screen.findByRole("heading", { name: "Edit collection" })
    expect(screen.getByText(/provider is unavailable/i)).toBeTruthy()
    expect(isDisabled(screen.getByRole("combobox", { name: "Media type" }))).toBe(true)
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Renamed" } })
    const save = screen.getByRole("button", { name: "Save" })
    expect(isDisabled(save)).toBe(false)
    fireEvent.click(save)

    await waitFor(() => expect(update).toHaveBeenCalledWith(
      current.id,
      expect.objectContaining({ title: "Renamed" }),
    ))
  })

  test("result-affecting edits require a fresh Preview", async () => {
    const current = profile("a".repeat(16), { limit: { kind: "maximum", count: 20 } })
    mockPage({ profiles: [current] })
    vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    const update = vi.spyOn(api.api.collections, "updateProfile").mockResolvedValue(current)
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })

    fireEvent.change(screen.getByRole("spinbutton", { name: "Maximum results" }), {
      target: { value: "21" },
    })
    const save = screen.getByRole("button", { name: "Save" })
    expect(isDisabled(save)).toBe(true)
    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    await screen.findByText("The Matrix (1999)")
    expect(isDisabled(save)).toBe(false)
    fireEvent.click(save)
    await waitFor(() => expect(update).toHaveBeenCalledTimes(1))
  })

  test("read-only configuration disables URL-opened edits and the template catalog", async () => {
    const current = profile("a".repeat(16))
    mockPage({
      settings: {
        ...accountSettings,
        access: { readOnly: true, version: 2 },
      },
      profiles: [current],
    })
    const update = vi.spyOn(api.api.collections, "updateProfile")
    page(`/settings/collections?edit=${current.id}`)

    await screen.findByRole("heading", { name: "Edit collection" })
    expect(isDisabled(screen.getByLabelText("Title"))).toBe(true)
    const save = screen.getByRole("button", { name: "Save" })
    expect(isDisabled(save)).toBe(true)
    fireEvent.click(save)
    expect(update).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Edit collection" })).toBeNull())
    const catalogButton = screen.getAllByRole("button").find((button) =>
      button.textContent?.includes("Popular movies from TMDB."),
    )
    if (!catalogButton) throw new Error("Expected the Popular movies template button")
    expect(isDisabled(catalogButton)).toBe(true)
  })

  test("provider availability does not leak from another account's template cache", async () => {
    mockPage({ templates: catalog(template({ title: "New account template" })) })
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    client.setQueryData(queryKeys.collectionTemplates("https://old.example:user-2"),
      catalog(template({ title: "Old account template" })))
    page("/settings/collections", client)

    expect(await screen.findByRole("button", { name: /New account template/ })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Old account template/ })).toBeNull()
  })

  test("the same template can be used for more than one collection", async () => {
    mockPage()
    vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    const create = vi.spyOn(api.api.collections, "createProfile")
      .mockResolvedValueOnce({ profile: profile("a".repeat(16)), total: 1 })
      .mockResolvedValueOnce({ profile: profile("c".repeat(16)), total: 1 })
    page()

    for (let index = 0; index < 2; index += 1) {
      await openTemplate()
      fireEvent.change(screen.getByLabelText("Title"), { target: { value: `Popular ${index + 1}` } })
      fireEvent.click(screen.getByRole("button", { name: "Preview" }))
      await screen.findByText("The Matrix (1999)")
      fireEvent.click(createButton())
      await waitFor(() => expect(create).toHaveBeenCalledTimes(index + 1))
      await waitFor(() => expect(screen.queryByRole("heading", { name: "Add collection" })).toBeNull())
    }
  })
})

describe("collection settings management", () => {
  test("reorders and deletes configured collections", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("c".repeat(16), { title: "Second" })
    mockPage({ profiles: [first, second] })
    const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockResolvedValue({
      profiles: [second, first],
    })
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockResolvedValue({ deleted: true })
    vi.spyOn(window, "confirm").mockReturnValue(true)
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Move First down" }))
    await waitFor(() => expect(reorder).toHaveBeenCalledWith([second.id, first.id]))
    fireEvent.click(screen.getByRole("button", { name: "Delete First" }))
    await waitFor(() => expect(remove).toHaveBeenCalledWith(first.id))
  })
})
