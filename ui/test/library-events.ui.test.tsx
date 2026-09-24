import { QueryClientProvider } from "@tanstack/react-query"
import { act, render } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
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

  function queryFor(queryKey: readonly unknown[]) {
    return queryClient.getQueryCache().build(queryClient, { queryKey })
  }

  function seed(...queryKeys: (readonly unknown[])[]) {
    for (const queryKey of queryKeys) queryClient.setQueryData(queryKey, {})
  }

  function invalidated(queryKey: readonly unknown[]) {
    return queryClient.getQueryState(queryKey)?.isInvalidated ?? false
  }

  function shellEvent(type: string, payload: { itemIds: string[]; contextIds?: string[] }) {
    window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", { detail: { type, payload } }))
  }

  test("a committed batch refreshes watch state and changed items but keeps the billboard", () => {
    seed(queryKeys.homeResume, queryKeys.item("series"), queryKeys.item("other"), queryKeys.billboard)
    render(<Bridge />)

    shellEvent("library-changed", { itemIds: ["episode"], contextIds: ["season", "series"] })

    expect(invalidated(queryKeys.homeResume)).toBe(true)
    expect(invalidated(queryKeys.item("series"))).toBe(true)
    expect(invalidated(queryKeys.item("other"))).toBe(false)
    expect(invalidated(queryKeys.billboard)).toBe(false)
  })

  test("catalog bursts never refresh live Next Up and flush their final aggregate state", () => {
    vi.useFakeTimers()
    seed(queryKeys.home, queryKeys.homeResume, queryKeys.item("second"))
    render(<Bridge />)

    shellEvent("catalog-changed", { itemIds: ["first"], contextIds: ["series"] })
    expect(invalidated(queryKeys.home)).toBe(true)
    expect(invalidated(queryKeys.homeResume)).toBe(false)

    seed(queryKeys.home)
    shellEvent("catalog-changed", { itemIds: ["second"], contextIds: ["series"] })
    expect(invalidated(queryKeys.item("second"))).toBe(true)
    expect(invalidated(queryKeys.home)).toBe(false)

    act(() => vi.advanceTimersByTime(1_000))
    expect(invalidated(queryKeys.home)).toBe(true)
    expect(invalidated(queryKeys.homeResume)).toBe(false)
  })

  test("sustained bootstrap pages refresh aggregates at most once per second", () => {
    vi.useFakeTimers()
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue()
    render(<Bridge />)
    for (let page = 0; page < 12; page += 1) {
      window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", {
        detail: { type: "catalog-changed", payload: { itemIds: [`item-${page}`] } },
      }))
      vi.advanceTimersByTime(250)
    }
    const homeRefreshes = invalidate.mock.calls.filter(([filter]) => filter?.predicate?.(queryFor(queryKeys.home)))
    expect(homeRefreshes).toHaveLength(4)
    vi.advanceTimersByTime(1_000)
    expect(invalidate.mock.calls.filter(([filter]) => filter?.predicate?.(queryFor(queryKeys.home)))).toHaveLength(4)
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
