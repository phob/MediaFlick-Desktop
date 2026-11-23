import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { MyCollectionDetail } from "../src/routes/CollectionDetail"
import type { CollectionSettings } from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { appStatus } from "./support/fixtures"
import { TestProviders } from "./test-utils"

type CollectionProfileDetail = Awaited<ReturnType<typeof api.api.collections.mineDetail>>

const PROFILE_ID = "a".repeat(16)

function collectionSettings(tmdb: boolean, mdblist = true): CollectionSettings {
  return {
    effectiveMode: "mediaFlick",
    mediaFlickAvailable: true,
    modeSelection: "mediaFlick",
    franchises: { includeUnreleased: false },
    readiness: { tmdb, mdblist },
    recovery: null,
  }
}

function manualDetail(patch: Partial<CollectionProfileDetail> = {}): CollectionProfileDetail {
  return {
    profile: {
      id: PROFILE_ID,
      revision: "b".repeat(16),
      template: { id: "tmdb.discover.movie.popular" },
      title: "Manual Favorites",
      description: "",
      customPosterId: null,
      source: { kind: "tmdbDiscover", parameters: {} },
      mediaType: "movie",
      limit: { kind: "all" },
      cadence: "manual",
      availableOnHome: false,
    },
    status: "updating",
    owned: [],
    missing: [],
    items: [],
    libraryItems: [],
    refresh: {
      lastAttempt: null,
      lastSuccess: null,
      latestFailure: null,
      nextDue: null,
      initialized: false,
    },
    ...patch,
  }
}

function renderDetail() {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-1",
  }))
  return render(
    <TestProviders client={client} initialEntries={[`/collections/mine/${PROFILE_ID}`]}>
      <Routes>
        <Route path="/collections/mine/:profileId" element={<MyCollectionDetail />} />
      </Routes>
    </TestProviders>,
  )
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe("MyCollectionDetail", () => {
  test("offers a Retry action instead of stranding an updating manual collection", async () => {
    const detail = vi.spyOn(api.api.collections, "mineDetail")
      .mockResolvedValueOnce(manualDetail({ status: "updating" }))
      .mockResolvedValueOnce(manualDetail({
        status: "ready",
        overdue: false,
        refresh: {
          lastAttempt: 1_700_000_000,
          lastSuccess: 1_700_000_000,
          latestFailure: null,
          nextDue: null,
          initialized: true,
        },
      }))
    vi.spyOn(api.api.collections, "settings").mockResolvedValue(collectionSettings(true))
    const refresh = vi.spyOn(api.api.collections, "refreshProfile").mockResolvedValue({
      profile: manualDetail().profile,
      total: 0,
    })

    renderDetail()

    const retry = await screen.findByRole("button", { name: "Retry" })
    expect(screen.getByText("Updating results")).toBeTruthy()
    fireEvent.click(retry)

    await waitFor(() => expect(refresh).toHaveBeenCalledWith(PROFILE_ID))
    await waitFor(() => expect(detail).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole("button", { name: "Retry" })).toBeNull())
  })

  test("leaves Updating results when the collection provider is unavailable", async () => {
    vi.spyOn(api.api.collections, "mineDetail").mockResolvedValue(manualDetail({ status: "updating" }))
    vi.spyOn(api.api.collections, "settings").mockResolvedValue(collectionSettings(false))

    renderDetail()

    await screen.findByText("Provider unavailable.")
    expect(screen.queryByText("Updating results")).toBeNull()
    expect(screen.getByText("Results unavailable")).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull()
  })
})
