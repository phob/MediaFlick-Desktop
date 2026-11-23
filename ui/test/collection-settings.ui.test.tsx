import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import type { ReactNode } from "react"
import { Route, Routes, useNavigate } from "react-router-dom"
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
import { appStatus } from "./support/fixtures"
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
    originalTitle: null,
    year: null,
    overview: "",
    releaseDate: null,
    sourceOrder: id,
    posterPath: null,
    backdropPath: null,
    adult: false,
    ...patch,
  }
}

function preview(
  items = [title(603, {
    title: "The Matrix",
    year: 1999,
    posterPath: "/matrix.jpg",
  })],
  sourceIdentity: string | null = null,
) {
  return {
    items,
    total: items.length,
    movies: items.filter((item) => item.mediaType === "movie").length,
    series: items.filter((item) => item.mediaType === "series").length,
    sourceIdentity,
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
  vi.spyOn(api.api.collections, "profiles").mockResolvedValue({
    profiles: options.profiles ?? [],
    errors: {},
  })
  vi.spyOn(api.api.collections, "templates").mockResolvedValue(
    options.templates ?? catalog(template()),
  )
}

function page(
  initialEntry = "/settings/collections",
  client = testQueryClient(),
) {
  client.setQueryData(queryKeys.status, appStatus({
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-1",
    userName: "Neo",
  }))
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
    fireEvent.click(screen.getByRole("button", { name: "Undo changes" }))
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
    fireEvent.click(screen.getByRole("button", { name: "Undo changes" }))
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

  test("blocks Escape, overlay clicks, and Close while a save is in flight", async () => {
    const current = profile("a".repeat(16), { title: "Original title" })
    mockPage({ profiles: [current] })
    let resolveSave!: (value: CollectionProfile) => void
    const update = vi.spyOn(api.api.collections, "updateProfile").mockReturnValue(
      new Promise((resolve) => { resolveSave = resolve }),
    )
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0))
    })

    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Saving title" } })
    fireEvent.click(createButton())
    await waitFor(() => expect(update).toHaveBeenCalledTimes(1))

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" })
    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()

    const overlay = document.querySelector('[data-slot="dialog-overlay"]')
    expect(overlay).toBeTruthy()
    if (overlay) {
      fireEvent.pointerDown(overlay, { button: 0, ctrlKey: false, pointerType: "mouse" })
      fireEvent.click(overlay)
    }
    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()

    await act(async () => {
      resolveSave({ ...current, title: "Saving title" })
    })
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Edit collection" })).toBeNull())
  })

  test("blocks Escape, overlay clicks, and Close while a poster upload is in flight", async () => {
    const current = profile("a".repeat(16), { title: "Poster upload test" })
    mockPage({ profiles: [current] })
    let finishUpload!: (value: { id: string }) => void
    const upload = vi.spyOn(api.api.collections, "uploadArtwork").mockReturnValue(
      new Promise((resolve) => { finishUpload = resolve }),
    )
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0))
    })

    const file = new File(["poster"], "poster.png", { type: "image/png" })
    Object.defineProperty(file, "arrayBuffer", { value: async () => new ArrayBuffer(6) })
    fireEvent.change(screen.getByLabelText("Custom poster file"), { target: { files: [file] } })
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(1))

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" })
    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()
    expect(screen.queryByRole("alertdialog")).toBeNull()

    const overlay = document.querySelector('[data-slot="dialog-overlay"]')
    expect(overlay).toBeTruthy()
    if (overlay) {
      fireEvent.pointerDown(overlay, { button: 0, ctrlKey: false, pointerType: "mouse" })
      fireEvent.click(overlay)
    }
    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()
    expect(screen.queryByRole("alertdialog")).toBeNull()

    const closeButton = screen.getByRole("button", { name: "Close" })
    expect(isDisabled(closeButton)).toBe(true)
    fireEvent.click(closeButton)
    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()
    expect(upload.mock.calls[0][1]?.aborted).toBe(false)

    await act(async () => finishUpload({ id: "d".repeat(32) }))
    expect(isDisabled(screen.getByRole("button", { name: "Close" }))).toBe(false)
  })

  test("prompts before discarding dirty wizard edits on Escape, overlay click, and Close", async () => {
    const current = profile("a".repeat(16), { title: "Existing collection" })
    mockPage({ profiles: [current] })
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" })
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Edit collection" })).toBeNull())
    expect(screen.queryByRole("alertdialog")).toBeNull()

    fireEvent.click(await screen.findByRole("button", { name: "Edit Existing collection" }))
    await screen.findByRole("heading", { name: "Edit collection" })
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Unsaved edit" } })
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0))
    })

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" })
    const escapePrompt = await screen.findByRole("alertdialog")
    fireEvent.click(within(escapePrompt).getByRole("button", { name: "Keep editing" }))
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
    expect((await screen.findByLabelText("Title") as HTMLInputElement).value).toBe("Unsaved edit")

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0))
    })
    const overlay = document.querySelector('[data-slot="dialog-overlay"]')
    expect(overlay).toBeTruthy()
    if (overlay) {
      fireEvent.pointerDown(overlay, { button: 0, ctrlKey: false, pointerType: "mouse" })
      fireEvent.click(overlay)
    }
    const overlayPrompt = await screen.findByRole("alertdialog")
    fireEvent.click(within(overlayPrompt).getByRole("button", { name: "Keep editing" }))
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
    expect((await screen.findByLabelText("Title") as HTMLInputElement).value).toBe("Unsaved edit")

    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    const closePrompt = await screen.findByRole("alertdialog")
    fireEvent.click(within(closePrompt).getByRole("button", { name: "Discard changes" }))
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
    expect(screen.queryByRole("heading", { name: "Edit collection" })).toBeNull()
  })

  test("stale save completion does not close a newly opened wizard", async () => {
    const first = profile("a".repeat(16), { title: "First collection" })
    const second = profile("c".repeat(16), { title: "Second collection" })
    mockPage({ profiles: [first, second] })
    let resolveFirstSave!: (value: CollectionProfile) => void
    const update = vi.spyOn(api.api.collections, "updateProfile").mockReturnValue(
      new Promise((resolve) => { resolveFirstSave = resolve }),
    )
    const client = testQueryClient()
    client.setQueryData(queryKeys.status, appStatus({
      authenticated: true,
      serverUrl: "https://jellyfin.example",
      userId: "user-1",
      userName: "Neo",
    }))

    let go!: ReturnType<typeof useNavigate>
    function Harness() {
      go = useNavigate()
      return <CollectionSettingsPage />
    }

    render(
      <TestProviders client={client} initialEntries={[`/settings/collections?edit=${first.id}`]}>
        <Routes><Route path="*" element={<Harness />} /></Routes>
      </TestProviders>,
    )

    await screen.findByRole("heading", { name: "Edit collection" })
    expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe("First collection")

    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "First edited" } })
    fireEvent.click(createButton())
    await waitFor(() => expect(update).toHaveBeenCalledTimes(1))

    await act(async () => {
      void go(`/settings/collections?edit=${second.id}`)
    })
    await waitFor(() => {
      expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe("Second collection")
    })
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Second edited" } })

    await act(async () => {
      resolveFirstSave({ ...first, title: "First edited" })
    })

    expect(screen.getByRole("heading", { name: "Edit collection" })).toBeTruthy()
    expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe("Second edited")
  })

  test("MDBList preview resolves sourceIdentity without overwriting concurrent draft edits", async () => {
    const mdblist = template({
      id: "mdblist.public.custom",
      title: "MDBList template",
      description: "Initial description",
      source: { kind: "mdbListPublicList", listId: "https://mdblist.com/lists/alice/top" },
      mediaType: "mixed",
      cadence: "daily",
    })
    mockPage({ templates: catalog(mdblist) })
    let resolvePreview!: (value: ReturnType<typeof preview>) => void
    vi.spyOn(api.api.collections, "preview").mockReturnValue(new Promise((resolve) => { resolvePreview = resolve }))
    const create = vi.spyOn(api.api.collections, "createProfile").mockResolvedValue({
      profile: profile("a".repeat(16)),
      total: 1,
    })
    page()
    await openTemplate("MDBList template")

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))

    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Concurrent title" } })
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), { target: { value: "Concurrent description" } })
    fireEvent.click(screen.getByRole("combobox", { name: "Refresh cadence" }))
    fireEvent.click(await screen.findByRole("option", { name: "Weekly" }))
    fireEvent.click(screen.getByRole("switch", { name: "Available on Home" }))

    await act(async () => resolvePreview(preview(undefined, "12345")))

    expect(await screen.findByText("The Matrix (1999)")).toBeTruthy()
    expect((screen.getByLabelText("MDBList public list ID or canonical URL") as HTMLInputElement).value).toBe("12345")
    expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe("Concurrent title")
    expect((screen.getByRole("textbox", { name: "Description" }) as HTMLTextAreaElement).value).toBe("Concurrent description")
    expect(screen.getByRole("combobox", { name: "Refresh cadence" }).textContent).toBe("Weekly")
    expect(screen.getByRole("switch", { name: "Available on Home" }).getAttribute("aria-checked")).toBe("true")

    fireEvent.click(createButton())
    await waitFor(() => expect(create).toHaveBeenCalledWith(expect.objectContaining({
      title: "Concurrent title",
      description: "Concurrent description",
      cadence: "weekly",
      availableOnHome: true,
      source: { kind: "mdbListPublicList", listId: "12345" },
    })))
  })

  test("changing MDBList URL or reopening the wizard discards in-flight preview sourceIdentity", async () => {
    const mdblist = template({
      id: "mdblist.public.custom",
      title: "MDBList template",
      source: { kind: "mdbListPublicList", listId: "https://mdblist.com/lists/alice/first" },
      mediaType: "mixed",
    })
    mockPage({ templates: catalog(mdblist) })
    const resolvers: Array<(value: ReturnType<typeof preview>) => void> = []
    vi.spyOn(api.api.collections, "preview").mockImplementation(
      () => new Promise((resolve) => { resolvers.push(resolve) }),
    )
    page()
    await openTemplate("MDBList template")

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    fireEvent.change(screen.getByLabelText("MDBList public list ID or canonical URL"), {
      target: { value: "https://mdblist.com/lists/bob/second" },
    })

    await act(async () => resolvers[0](preview(undefined, "11111")))

    expect((screen.getByLabelText("MDBList public list ID or canonical URL") as HTMLInputElement).value).toBe("https://mdblist.com/lists/bob/second")
    expect(screen.queryByText("The Matrix (1999)")).toBeNull()

    fireEvent.click(screen.getByRole("button", { name: "Preview" }))
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    const confirm = await screen.findByRole("alertdialog")
    fireEvent.click(within(confirm).getByRole("button", { name: "Discard changes" }))
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Add collection" })).toBeNull())

    await openTemplate("MDBList template")
    await act(async () => resolvers[1](preview(undefined, "22222")))

    expect((screen.getByLabelText("MDBList public list ID or canonical URL") as HTMLInputElement).value).toBe("https://mdblist.com/lists/alice/first")
    expect(screen.queryByText("The Matrix (1999)")).toBeNull()
  })

  test("Preview button is disabled while a poster upload is in progress", async () => {
    const current = profile("a".repeat(16))
    mockPage({ profiles: [current] })
    let finishUpload!: (value: { id: string }) => void
    const upload = vi.spyOn(api.api.collections, "uploadArtwork").mockReturnValue(
      new Promise((resolve) => { finishUpload = resolve }),
    )
    page(`/settings/collections?edit=${current.id}`)
    await screen.findByRole("heading", { name: "Edit collection" })

    const previewButton = screen.getByRole("button", { name: "Preview" })
    expect(isDisabled(previewButton)).toBe(false)

    const file = new File(["poster"], "poster.png", { type: "image/png" })
    Object.defineProperty(file, "arrayBuffer", { value: async () => new ArrayBuffer(6) })
    fireEvent.change(screen.getByLabelText("Custom poster file"), { target: { files: [file] } })
    await waitFor(() => expect(upload).toHaveBeenCalledTimes(1))

    expect(isDisabled(previewButton)).toBe(true)

    await act(async () => finishUpload({ id: "e".repeat(32) }))
    expect(isDisabled(previewButton)).toBe(false)
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
      includeUnreleased: true,
    }))
  })

  test("toggling Include unreleased in automatic mode does not send modeSelection", async () => {
    const automaticJellyfin: CollectionSettings = {
      effectiveMode: "jellyfin",
      mediaFlickAvailable: false,
      modeSelection: null,
      franchises: { includeUnreleased: false },
      readiness: { tmdb: false, mdblist: false },
      recovery: null,
    }
    mockPage({ settings: automaticJellyfin })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
      ...automaticJellyfin,
      franchises: { includeUnreleased: true },
    })
    page()

    fireEvent.click(await screen.findByRole("switch", { name: "Include unreleased titles" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(patch).toHaveBeenCalledTimes(1))
    expect(patch).toHaveBeenCalledWith({ includeUnreleased: true })
    expect(patch.mock.calls[0][0]).not.toHaveProperty("modeSelection")
  })

  test("stages collection ordering until Save and skips settings PATCH when unchanged", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("c".repeat(16), { title: "Second" })
    mockPage({
      settings: { ...accountSettings, modeSelection: null },
      profiles: [first, second],
    })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
      ...accountSettings,
      modeSelection: null,
    })
    const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockResolvedValue({
      profiles: [second, first],
      errors: {},
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Move First down" }))
    expect(reorder).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(reorder).toHaveBeenCalledWith([second.id, first.id]))
    expect(patch).not.toHaveBeenCalled()
  })

  test("deleting the last profile while TMDB is not ready skips settings PATCH and clears dirty state", async () => {
    const only = profile("a".repeat(16), { title: "Only collection" })
    const beforeDelete: CollectionSettings = {
      effectiveMode: "mediaFlick",
      mediaFlickAvailable: true,
      modeSelection: null,
      franchises: { includeUnreleased: false },
      readiness: { tmdb: false, mdblist: true },
      recovery: null,
    }
    const afterDelete: CollectionSettings = {
      ...beforeDelete,
      effectiveMode: "jellyfin",
      mediaFlickAvailable: false,
    }
    let deleted = false
    vi.spyOn(api.api.collections, "settings").mockImplementation(async () =>
      deleted ? afterDelete : beforeDelete,
    )
    vi.spyOn(api.api.collections, "profiles").mockImplementation(async () => ({
      profiles: deleted ? [] : [only],
      errors: {},
    }))
    vi.spyOn(api.api.collections, "templates").mockResolvedValue(catalog(template()))
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockImplementation(async () => {
      deleted = true
      return { deleted: true }
    })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockRejectedValue(
      new api.ApiError("MediaFlick collections are unavailable", 409),
    )
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Delete Only collection" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(remove).toHaveBeenCalledWith(only.id))
    await waitFor(() => expect(screen.getByText("No collections are configured.")).toBeTruthy())
    expect(patch).not.toHaveBeenCalled()
    expect(screen.queryByText("You have unsaved changes.")).toBeNull()
    expect(screen.getByRole("combobox", { name: "Collection mode" }).textContent).toBe("Jellyfin")
  })

  test("explicitly changing collection mode sends only modeSelection", async () => {
    const automatic: CollectionSettings = {
      ...accountSettings,
      modeSelection: null,
    }
    mockPage({ settings: automatic })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
      ...automatic,
      effectiveMode: "jellyfin",
      modeSelection: "jellyfin",
    })
    page()

    const modeSelect = await screen.findByRole("combobox", { name: "Collection mode" })
    await waitFor(() => expect(modeSelect.hasAttribute("disabled")).toBe(false))
    fireEvent.click(modeSelect)
    fireEvent.click(await screen.findByRole("option", { name: "Jellyfin" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(patch).toHaveBeenCalledTimes(1))
    expect(patch).toHaveBeenCalledWith({ modeSelection: "jellyfin" })
    expect(patch.mock.calls[0][0]).not.toHaveProperty("includeUnreleased")
  })

  test("creating a collection while reorder draft is dirty keeps the new collection visible and saves the complete order", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("b".repeat(16), { title: "Second" })
    const third = profile("c".repeat(16), { title: "Third" })
    let serverProfiles = [first, second]
    vi.spyOn(api.api.collections, "settings").mockResolvedValue({
      ...accountSettings,
      modeSelection: null,
    })
    vi.spyOn(api.api.collections, "profiles").mockImplementation(async () => ({
      profiles: [...serverProfiles],
      errors: {},
    }))
    vi.spyOn(api.api.collections, "templates").mockResolvedValue(catalog(template({ title: "Third" })))
    vi.spyOn(api.api.collections, "preview").mockResolvedValue(preview())
    vi.spyOn(api.api.collections, "createProfile").mockImplementation(async () => {
      serverProfiles = [...serverProfiles, third]
      return { profile: third, total: 1 }
    })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
      ...accountSettings,
      modeSelection: null,
    })
    const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockImplementation(async (ids) => {
      if (ids.length !== serverProfiles.length) {
        throw new api.ApiError("the collection order is incomplete", 400)
      }
      const byId = new Map(serverProfiles.map((item) => [item.id, item]))
      serverProfiles = ids.map((id) => byId.get(id)).filter((item): item is CollectionProfile => Boolean(item))
      return { profiles: [...serverProfiles], errors: {} }
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Move First down" }))
    await openTemplate("Third")
    const dialog = screen.getByRole("dialog", { name: "Add collection" })
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }))
    await within(dialog).findByText("The Matrix (1999)")
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }))
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add collection" })).toBeNull())

    expect(await screen.findByRole("button", { name: "Edit Third" })).toBeTruthy()
    const rowButtons = screen.getAllByRole("button", { name: /^Edit / }).map((button) => button.getAttribute("aria-label"))
    expect(rowButtons).toEqual(["Edit Second", "Edit First", "Edit Third"])

    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(reorder).toHaveBeenCalledWith([second.id, first.id, third.id]))
    expect(patch).not.toHaveBeenCalled()
    await waitFor(() => expect(screen.queryByText("You have unsaved changes.")).toBeNull())
  })

  test("reorder runs before destructive deletes and settings PATCH on Save", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("b".repeat(16), { title: "Second" })
    const third = profile("c".repeat(16), { title: "Third" })
    mockPage({ profiles: [first, second, third] })
    const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockRejectedValue(
      new api.ApiError("the collection order is incomplete", 400),
    )
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockResolvedValue({ deleted: true })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
      ...accountSettings,
      franchises: { includeUnreleased: true },
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Delete First" }))
    fireEvent.click(screen.getByRole("button", { name: "Move Second down" }))
    fireEvent.click(screen.getByRole("switch", { name: "Include unreleased titles" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(reorder).toHaveBeenCalledWith([first.id, third.id, second.id]))
    expect(remove).not.toHaveBeenCalled()
    expect(patch).not.toHaveBeenCalled()
    expect(screen.getByRole("button", { name: "Undo deletion of First" })).toBeTruthy()
  })

  test("partial Save failure after one delete clears the committed delete and retries only the remaining delete", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("b".repeat(16), { title: "Second" })
    const third = profile("c".repeat(16), { title: "Third" })
    let serverProfiles = [first, second, third]
    let failSecond = true
    vi.spyOn(api.api.collections, "settings").mockResolvedValue(accountSettings)
    vi.spyOn(api.api.collections, "profiles").mockImplementation(async () => ({
      profiles: [...serverProfiles],
      errors: {},
    }))
    vi.spyOn(api.api.collections, "templates").mockResolvedValue(catalog(template()))
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockImplementation(async (id) => {
      if (!serverProfiles.some((item) => item.id === id)) {
        throw new api.ApiError("that collection does not exist", 404)
      }
      if (id === second.id && failSecond) {
        failSecond = false
        throw new api.ApiError("disk busy", 500)
      }
      serverProfiles = serverProfiles.filter((item) => item.id !== id)
      return { deleted: true }
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Delete First" }))
    fireEvent.click(screen.getByRole("button", { name: "Delete Second" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(remove).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole("button", { name: "Undo deletion of First" })).toBeNull())
    expect(screen.getByRole("button", { name: "Undo deletion of Second" })).toBeTruthy()
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Undo deletion of Second" }))
    await waitFor(() => expect(screen.queryByText("You have unsaved changes.")).toBeNull())

    fireEvent.click(screen.getByRole("button", { name: "Delete Second" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(remove).toHaveBeenCalledTimes(3))
    expect(remove.mock.calls.map((call) => call[0])).toEqual([first.id, second.id, second.id])
    await waitFor(() => expect(screen.queryByText("You have unsaved changes.")).toBeNull())
  })

  test("partial Save failure after the only delete succeeds leaves no phantom unsaved changes", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("b".repeat(16), { title: "Second" })
    let serverProfiles = [first, second]
    let failSettings = true
    vi.spyOn(api.api.collections, "settings").mockResolvedValue(accountSettings)
    vi.spyOn(api.api.collections, "profiles").mockImplementation(async () => ({
      profiles: [...serverProfiles],
      errors: {},
    }))
    vi.spyOn(api.api.collections, "templates").mockResolvedValue(catalog(template()))
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockImplementation(async (id) => {
      serverProfiles = serverProfiles.filter((item) => item.id !== id)
      return { deleted: true }
    })
    const patch = vi.spyOn(api.api.collections, "patchSettings").mockImplementation(async (body) => {
      if (failSettings) {
        failSettings = false
        throw new api.ApiError("settings write failed", 500)
      }
      return { ...accountSettings, franchises: { includeUnreleased: Boolean(body.includeUnreleased) } }
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Delete First" }))
    fireEvent.click(screen.getByRole("switch", { name: "Include unreleased titles" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(remove).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(patch).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByRole("button", { name: "Undo deletion of First" })).toBeNull())
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    fireEvent.click(screen.getByRole("switch", { name: "Include unreleased titles" }))
    await waitFor(() => expect(screen.queryByText("You have unsaved changes.")).toBeNull())
  })

  test("partial Save failure after reorder succeeds and delete fails does not repeat reorder on retry", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("b".repeat(16), { title: "Second" })
    const third = profile("c".repeat(16), { title: "Third" })
    let serverProfiles = [first, second, third]
    let failDelete = true
    vi.spyOn(api.api.collections, "settings").mockResolvedValue(accountSettings)
    vi.spyOn(api.api.collections, "profiles").mockImplementation(async () => ({
      profiles: [...serverProfiles],
      errors: {},
    }))
    vi.spyOn(api.api.collections, "templates").mockResolvedValue(catalog(template()))
    const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockImplementation(async (ids) => {
      const byId = new Map(serverProfiles.map((item) => [item.id, item]))
      serverProfiles = ids.map((id) => byId.get(id)).filter((item): item is CollectionProfile => Boolean(item))
      return { profiles: [...serverProfiles], errors: {} }
    })
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockImplementation(async (id) => {
      if (failDelete) {
        failDelete = false
        throw new api.ApiError("delete failed", 500)
      }
      serverProfiles = serverProfiles.filter((item) => item.id !== id)
      return { deleted: true }
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Delete First" }))
    fireEvent.click(screen.getByRole("button", { name: "Move Second down" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(reorder).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(remove).toHaveBeenCalledTimes(1))
    expect(screen.getByRole("button", { name: "Undo deletion of First" })).toBeTruthy()
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(remove).toHaveBeenCalledTimes(2))
    expect(reorder).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByText("You have unsaved changes.")).toBeNull())
  })

  test("partial Save failure when follow-up settings refresh fails after delete leaves no phantom dirty state", async () => {
    const first = profile("a".repeat(16), { title: "First" })
    const second = profile("b".repeat(16), { title: "Second" })
    let serverProfiles = [first, second]
    let failRefresh = false
    vi.spyOn(api.api.collections, "settings").mockImplementation(async () => {
      if (failRefresh) {
        failRefresh = false
        throw new api.ApiError("settings refresh failed", 500)
      }
      return accountSettings
    })
    vi.spyOn(api.api.collections, "profiles").mockImplementation(async () => ({
      profiles: [...serverProfiles],
      errors: {},
    }))
    vi.spyOn(api.api.collections, "templates").mockResolvedValue(catalog(template()))
    const remove = vi.spyOn(api.api.collections, "deleteProfile").mockImplementation(async (id) => {
      serverProfiles = serverProfiles.filter((item) => item.id !== id)
      failRefresh = true
      return { deleted: true }
    })
    page()

    fireEvent.click(await screen.findByRole("button", { name: "Delete First" }))
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(remove).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByText("You have unsaved changes.")).toBeNull())
    expect(screen.queryByRole("button", { name: "Undo deletion of First" })).toBeNull()
  })
})

test("signed-out users see the sign-in prompt instead of being redirected away", () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: false }))
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
