import { render } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
import { useLibraryMetadataBridge } from "../src/lib/library-events"
import {
  invalidateMediaSurfaces,
  queryClient,
  queryKeys,
} from "../src/lib/query-client"

function Bridge() {
  useLibraryMetadataBridge()
  return null
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

  test("invalidates one active batch including item and context ids", () => {
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue()
    render(<Bridge />)

    window.dispatchEvent(
      new CustomEvent("mediaflick-desktop-shell", {
        detail: {
          type: "library-changed",
          payload: { itemIds: ["episode"], contextIds: ["season", "series"] },
        },
      }),
    )

    expect(invalidate).toHaveBeenCalledTimes(1)
    const filters = invalidate.mock.calls[0]?.[0]
    expect(filters?.refetchType).toBe("active")
    expect(filters?.predicate?.(queryFor(queryKeys.homeResume))).toBe(true)
    expect(filters?.predicate?.(queryFor(["item", "series"]))).toBe(true)
    expect(filters?.predicate?.(queryFor(["item", "other"]))).toBe(false)
    expect(filters?.predicate?.(queryFor(["status"]))).toBe(true)
    expect(filters?.predicate?.(queryFor(["collections", "account", "mine", "profile"]))).toBe(true)
    expect(filters?.predicate?.(queryFor(queryKeys.billboard))).toBe(false)
  })

  test("catalog bursts preserve live Next Up and flush their final aggregate state", () => {
    vi.useFakeTimers()
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue()
    const bridge = render(<Bridge />)
    const page = (id: string) => window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", {
      detail: { type: "catalog-changed", payload: { itemIds: [id], contextIds: ["series"] } },
    }))
    page("first")
    const first = invalidate.mock.calls[0]?.[0]?.predicate
    expect(first?.(queryFor(queryKeys.home))).toBe(true)
    expect(first?.(queryFor(queryKeys.homeResume))).toBe(false)
    expect(first?.(queryFor(queryKeys.homeSettings))).toBe(false)
    page("second")
    const second = invalidate.mock.calls[1]?.[0]?.predicate
    expect(second?.(queryFor(queryKeys.home))).toBe(false)
    expect(second?.(queryFor(queryKeys.item("second")))).toBe(true)
    expect(second?.(queryFor(queryKeys.children("series")))).toBe(true)
    vi.advanceTimersByTime(1_000)
    expect(invalidate).toHaveBeenCalledTimes(3)
    const final = invalidate.mock.calls[2]?.[0]?.predicate
    expect(final?.(queryFor(queryKeys.home))).toBe(true)
    expect(final?.(queryFor(queryKeys.homeResume))).toBe(false)
    page("third")
    page("fourth")
    bridge.unmount()
    vi.advanceTimersByTime(1_000)
    expect(invalidate).toHaveBeenCalledTimes(5)
  })

  test("sync completion refreshes live watch state once and consumes the trailing catalog refresh", () => {
    vi.useFakeTimers()
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue()
    render(<Bridge />)
    for (const type of ["catalog-changed", "catalog-changed", "library-changed"]) {
      window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", {
        detail: { type, payload: { itemIds: [], contextIds: [] } },
      }))
    }
    expect(invalidate.mock.calls[2]?.[0]?.predicate?.(queryFor(queryKeys.homeResume))).toBe(true)
    vi.advanceTimersByTime(1_000)
    expect(invalidate).toHaveBeenCalledTimes(3)
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
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue()

    invalidateMediaSurfaces("episode", "series")

    const filters = invalidate.mock.calls.map(([filter]) => filter)
    const filterFor = (queryKey: readonly unknown[]) =>
      filters.find((filter) => JSON.stringify(filter?.queryKey) === JSON.stringify(queryKey))

    expect(filterFor(queryKeys.item("episode"))?.exact).toBe(true)
    expect(filterFor(queryKeys.children("series"))?.exact).toBe(true)
    expect(filterFor(queryKeys.nextUp("series"))?.exact).toBe(true)
    expect(filterFor(queryKeys.billboard)).toBeUndefined()
    for (const untouched of [
      queryKeys.itemAbout("episode"),
      queryKeys.itemSynopsis("episode"),
      queryKeys.media("episode"),
      queryKeys.trailer("episode"),
    ]) {
      expect(filterFor(untouched)).toBeUndefined()
    }
  })
})
