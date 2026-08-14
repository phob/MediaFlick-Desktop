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
  afterEach(() => vi.restoreAllMocks())

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
    expect(filters?.predicate?.({ queryKey: ["item", "series"] } as never)).toBe(true)
    expect(filters?.predicate?.({ queryKey: ["item", "other"] } as never)).toBe(false)
    expect(filters?.predicate?.({ queryKey: ["status"] } as never)).toBe(true)
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
    expect(filterFor(queryKeys.billboard)?.exact).toBe(true)
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
