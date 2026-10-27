import { QueryClientProvider, type QueryKey } from "@tanstack/react-query"
import { act, render } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
import type * as Wire from "../src/lib/api/wire.gen"
import { useLibraryMetadataBridge } from "../src/lib/library-events"
import {
  createQueryClient,
  invalidateMediaSurfaces,
  queryKeys,
} from "../src/lib/query-client"

const queryClient = createQueryClient()

function MetadataBridge() {
  useLibraryMetadataBridge()
  return null
}

function Bridge() {
  return (
    <QueryClientProvider client={queryClient}>
      <MetadataBridge />
    </QueryClientProvider>
  )
}

describe("native library change bridge", () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    queryClient.clear()
  })

  function seed(...queryKeys: QueryKey[]) {
    for (const queryKey of queryKeys) queryClient.setQueryData(queryKey, {})
  }

  function invalidated(queryKey: QueryKey) {
    return queryClient.getQueryState(queryKey)?.isInvalidated ?? false
  }

  function shellEvent(detail: Wire.ShellEvent) {
    window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", { detail }))
  }

  test("a committed batch refreshes watch state and changed items but keeps the billboard", () => {
    seed(queryKeys.homeResume, queryKeys.item("series"), queryKeys.item("other"), queryKeys.billboard)
    render(<Bridge />)

    shellEvent({ type: "library-changed", payload: { itemIds: ["episode"], contextIds: ["season", "series"] } })

    expect(invalidated(queryKeys.homeResume)).toBe(true)
    expect(invalidated(queryKeys.item("series"))).toBe(true)
    expect(invalidated(queryKeys.item("other"))).toBe(false)
    expect(invalidated(queryKeys.billboard)).toBe(false)
  })

  test("catalog bursts never refresh live Next Up and flush their final aggregate state", () => {
    vi.useFakeTimers()
    seed(queryKeys.home, queryKeys.homeResume, queryKeys.item("second"))
    render(<Bridge />)

    shellEvent({ type: "catalog-changed", payload: { itemIds: ["first"], contextIds: ["series"], libraryIds: ["movies"] } })
    expect(invalidated(queryKeys.home)).toBe(true)
    expect(invalidated(queryKeys.homeResume)).toBe(false)

    seed(queryKeys.home)
    shellEvent({ type: "catalog-changed", payload: { itemIds: ["second"], contextIds: ["series"], libraryIds: ["movies"] } })
    expect(invalidated(queryKeys.item("second"))).toBe(true)
    expect(invalidated(queryKeys.home)).toBe(false)

    act(() => vi.advanceTimersByTime(1_000))
    expect(invalidated(queryKeys.home)).toBe(true)
    expect(invalidated(queryKeys.homeResume)).toBe(false)
  })

  test("user-state changes leave rich and technical item queries cached", () => {
    const untouched = [
      queryKeys.itemAbout("episode"),
      queryKeys.itemSynopsis("episode"),
      queryKeys.media("episode"),
      queryKeys.trailer("episode"),
      queryKeys.billboard,
    ]
    const refreshed = [queryKeys.item("episode"), queryKeys.children("series"), queryKeys.nextUp("series")]
    seed(...refreshed, ...untouched)

    invalidateMediaSurfaces(queryClient, "episode", "series")

    for (const queryKey of refreshed) expect(invalidated(queryKey)).toBe(true)
    for (const queryKey of untouched) expect(invalidated(queryKey)).toBe(false)
  })
})
