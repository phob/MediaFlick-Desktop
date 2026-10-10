import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import CollectionSettingsPage from "../src/routes/settings/CollectionSettings"
import type {
  CollectionProfile,
  CollectionSettings,
  CollectionTemplate,
  CollectionTemplates,
  NormalizedCollectionTitle,
} from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const accountSettings: CollectionSettings = {
  effectiveMode: "mediaFlick",
  mediaFlickAvailable: true,
  modeSelection: "mediaFlick",
  franchises: { includeUnreleased: false },
  readiness: { tmdb: true, mdblist: true },
  recovery: null,
}

function template(patch: Partial<CollectionTemplate> = {}): CollectionTemplate {
  return {
    id: "tmdb.discover.movie.popular",
    title: "Popular movies",
    description: "Popular movies from TMDB.",
    category: "popular",
    pictogram: "star",
    source: { kind: "tmdbDiscover", parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    cadence: "daily",
    ...patch,
  }
}

function profile(id: string, patch: Partial<CollectionProfile> = {}): CollectionProfile {
  return {
    id,
    revision: "b".repeat(16),
    template: { id: "tmdb.discover.movie.popular" },
    title: "Popular movies",
    description: "Popular movies from TMDB.",
    customPosterId: null,
    source: { kind: "tmdbDiscover", parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    cadence: "daily",
    availableOnHome: false,
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
  profiles?: CollectionProfile[]
  templates?: CollectionTemplates
} = {}) {
  vi.spyOn(api.api.collections, "settings").mockResolvedValue(accountSettings)
  vi.spyOn(api.api.collections, "profiles").mockResolvedValue({ profiles: options.profiles ?? [] })
  vi.spyOn(api.api.collections, "templates").mockResolvedValue(
    options.templates ?? catalog(template()),
  )
}

function page(
  initialEntry = "/settings/collections",
  client = testQueryClient(),
) {
  client.setQueryData(queryKeys.status, {
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-1",
    userName: "Neo",
  })
  const providers = ({ children }: { children: ReactNode }) => (
    <TestProviders client={client} initialEntries={[initialEntry]}>
        <Routes><Route path="*" element={children} /></Routes>
    </TestProviders>
  )
  return { client, ...render(<CollectionSettingsPage />, { wrapper: providers }) }
}

async function openTemplate(name = "Popular movies") {
  fireEvent.click(await screen.findByRole("button", { name: new RegExp(`^${name}`) }))
  await screen.findByRole("heading", { name: "Add collection" })
}

function createButton() {
  return screen.getByRole("button", { name: "Save" })
}

function isDisabled(element: Element) {
  return element.hasAttribute("disabled")
}

afterEach(() => vi.restoreAllMocks())

describe("collection settings wizard", () => {
  test("Save waits for poster upload and includes the selected poster", async () => {
    const current = profile("a".repeat(16))
    mockPage({ profiles: [current] })
    let finish!: (value: { id: string }) => void
    const upload = vi.spyOn(api.api.collections, "uploadArtwork").mockReturnValue(new Promise((resolve) => { finish = resolve }))
    const update = vi.spyOn(api.api.collections, "updateProfile").mockResolvedValue(current)
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })
    const file = new File(["poster"], "poster.png", { type: "image/png" })
    Object.defineProperty(file, "arrayBuffer", { value: async () => new ArrayBuffer(6) })
    fireEvent.change(screen.getByLabelText("Custom poster file"), { target: { files: [file] } })
    expect(isDisabled(createButton())).toBe(true)
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(1))
    await act(async () => finish({ id: "c".repeat(32) }))
    expect(isDisabled(createButton())).toBe(false)
    fireEvent.click(createButton())
    await waitFor(() => expect(update).toHaveBeenCalledWith(current.id, expect.objectContaining({ customPosterId: "c".repeat(32) })))
  })

  test("Reset cancels a poster upload and ignores its late response", async () => {
    const current = profile("a".repeat(16), { customPosterId: "b".repeat(32) })
    mockPage({ profiles: [current] })
    let finish!: (value: { id: string }) => void
    const upload = vi.spyOn(api.api.collections, "uploadArtwork").mockReturnValue(new Promise((resolve) => { finish = resolve }))
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })
    const file = new File(["poster"], "replacement.png", { type: "image/png" })
    Object.defineProperty(file, "arrayBuffer", { value: async () => new ArrayBuffer(6) })
    fireEvent.change(screen.getByLabelText("Custom poster file"), { target: { files: [file] } })
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole("button", { name: "Reset" }))
    expect(upload.mock.calls[0][1]?.aborted).toBe(true)
    await act(async () => finish({ id: "c".repeat(32) }))
    expect(screen.getByRole("img", { name: "Selected collection poster" }).getAttribute("src")).toContain(current.customPosterId)
    expect(isDisabled(createButton())).toBe(true)
  })

  test("resets and discards an unsaved collection draft", async () => {
    mockPage()
    page()
    await openTemplate()

    const title = screen.getByLabelText("Title")
    const description = screen.getByRole("textbox", { name: "Description" })
    fireEvent.change(description, { target: { value: "First line\nSecond line" } })
    fireEvent.change(title, { target: { value: "Changed title" } })
    fireEvent.click(screen.getByRole("button", { name: "Reset" }))
    expect((title as HTMLInputElement).value).toBe("Popular movies")
    expect((description as HTMLTextAreaElement).value).toBe(template().description)
    fireEvent.click(screen.getByRole("button", { name: "Discard" }))
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Add collection" })).toBeNull())
  })

  test("the first Save previews a new collection and the second Save creates it", async () => {
    mockPage()
    const runPreview = vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    const create = vi.spyOn(api.api.collections, "createProfile").mockResolvedValue({
      profile: profile("a".repeat(16)),
      total: 1,
    })
    page()
    await openTemplate()

    fireEvent.click(createButton())

    expect(await screen.findByText("The Matrix (1999)")).toBeTruthy()
    expect(runPreview).toHaveBeenCalledTimes(1)
    expect(create).not.toHaveBeenCalled()

    fireEvent.click(createButton())
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1))
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
  })

  test("source parameters, media type, and result limit each invalidate Preview", async () => {
    const custom = template({
      id: "tmdb.discover.movie.custom-discover",
      title: "Custom discover",
      source: { kind: "tmdbDiscover", parameters: {} },
      limit: { kind: "maximum", count: 20 },
    })
    mockPage({ templates: catalog(custom) })
    vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    page()
    await openTemplate("Custom discover")

    const runPreview = async () => {
      fireEvent.click(screen.getByRole("button", { name: /^Preview/ }))
      await screen.findByText("The Matrix (1999)")
    }
    const expectPreviewCleared = () => expect(screen.queryByText("The Matrix (1999)")).toBeNull()

    await runPreview()
    fireEvent.change(screen.getByLabelText("Metadata language (optional)"), {
      target: { value: "de-DE" },
    })
    expectPreviewCleared()

    await runPreview()
    fireEvent.change(screen.getByRole("spinbutton", { name: "Maximum results" }), {
      target: { value: "25" },
    })
    expectPreviewCleared()

    await runPreview()
    fireEvent.click(screen.getByRole("combobox", { name: "Media type" }))
    fireEvent.click(await screen.findByRole("option", { name: "Series" }))
    expectPreviewCleared()
  })

  test("provider availability does not leak from another account's template cache", async () => {
    mockPage({ templates: catalog(template({ title: "New account template" })) })
    const client = testQueryClient()
    client.setQueryData(queryKeys.collectionTemplates("https://old.example:user-2"),
      catalog(template({ title: "Old account template" })))
    page("/settings/collections", client)

    expect(await screen.findByRole("button", { name: /New account template/ })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /Old account template/ })).toBeNull()
  })
})

describe("collection settings management", () => {
  test("stages general changes behind Save and supports Discard", async () => {
    mockPage()
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
      ...accountSettings,
      franchises: { includeUnreleased: true },
    })
    page()

    const includeUnreleased = await screen.findByRole("switch", { name: "Include unreleased titles" })
    fireEvent.click(includeUnreleased)
    expect(patch).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Discard" }))
    expect(includeUnreleased.getAttribute("aria-checked")).toBe("false")

    fireEvent.click(includeUnreleased)
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(patch).toHaveBeenCalledWith({
      modeSelection: "mediaFlick",
      includeUnreleased: true,
    }))
  })

  test("stages collection ordering until Save", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("c".repeat(16), { title: "Second" })
    mockPage({ profiles: [first, second] })
    vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue(accountSettings)
    const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockResolvedValue({
      profiles: [second, first],
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Move First down" }))
    expect(reorder).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(reorder).toHaveBeenCalledWith([second.id, first.id]))
  })
})

test("signed-out users see the sign-in prompt instead of being redirected away", () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, { authenticated: false })
  render(
    <TestProviders client={client} initialEntries={["/settings/collections"]}>
      <Routes>
        <Route path="/settings/collections" element={<CollectionSettingsPage />} />
        <Route path="*" element={<p>Redirected</p>} />
      </Routes>
    </TestProviders>,
  )
  expect(screen.getByRole("heading", { name: "Collections" })).toBeTruthy()
  expect(screen.getByText("Sign in required")).toBeTruthy()
  expect(screen.queryByText("Redirected")).toBeNull()
})
