import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import * as api from "../src/lib/api"
import type { CollectionProfile, CollectionSettings, CollectionTemplates } from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import CollectionSettingsPage from "../src/routes/settings/CollectionSettings"
import { appStatus } from "./support/fixtures"
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

const emptyTemplates: CollectionTemplates = {
  categories: ["popular"],
  templates: [],
  readiness: { tmdb: true, mdblist: true },
}

function profile(id: string, title: string): CollectionProfile {
  return {
    id,
    revision: "b".repeat(16),
    template: { id: "tmdb.discover.movie.popular" },
    title,
    description: "",
    customPosterId: null,
    source: { kind: "tmdbDiscover", parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    cadence: "daily",
    availableOnHome: false,
  }
}

afterEach(() => vi.restoreAllMocks())

test("switching accounts mid-save aborts remaining collection mutations instead of mutating the new account", async () => {
  const first = profile("a".repeat(16), "First")
  const second = profile("b".repeat(16), "Second")
  const third = profile("c".repeat(16), "Third")

  let resolveReorder: ((value: api.CollectionProfilesIndex) => void) | null = null
  vi.spyOn(api.api.collections, "settings").mockResolvedValue(accountSettings)
  vi.spyOn(api.api.collections, "profiles").mockResolvedValue({
    profiles: [first, second, third],
    errors: {},
  })
  vi.spyOn(api.api.collections, "templates").mockResolvedValue(emptyTemplates)
  const reorder = vi.spyOn(api.api.collections, "reorderProfiles").mockImplementation(
    () => new Promise((resolve) => {
      resolveReorder = resolve
    }),
  )
  const remove = vi.spyOn(api.api.collections, "deleteProfile").mockResolvedValue({ deleted: true })
  const patch = vi.spyOn(api.api.collections, "patchSettings").mockResolvedValue({
    ...accountSettings,
    franchises: { includeUnreleased: true },
  })

  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({
    authenticated: true,
    serverUrl: "http://jellyfin.test",
    userId: "alice",
  }))

  render(
    <TestProviders client={client} initialEntries={["/settings/collections"]}>
      <Routes>
        <Route path="/settings/collections" element={<CollectionSettingsPage />} />
      </Routes>
    </TestProviders>,
  )

  fireEvent.click(await screen.findByRole("button", { name: "Delete First" }))
  fireEvent.click(screen.getByRole("button", { name: "Move Second down" }))
  fireEvent.click(screen.getByRole("switch", { name: "Include unreleased titles" }))
  fireEvent.click(screen.getByRole("button", { name: "Save" }))

  await waitFor(() => expect(reorder).toHaveBeenCalledTimes(1))

  act(() => {
    client.setQueryData(queryKeys.status, appStatus({
      authenticated: true,
      serverUrl: "http://jellyfin.test",
      userId: "bob",
    }))
    resolveReorder?.({ profiles: [first, third, second], errors: {} })
  })

  await waitFor(() => expect(screen.queryByRole("button", { name: "Saving…" })).toBeNull())
  expect(remove).not.toHaveBeenCalled()
  expect(patch).not.toHaveBeenCalled()
})
