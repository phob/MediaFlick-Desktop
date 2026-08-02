import { render } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
import { useLibraryMetadataBridge } from "../src/lib/library-events"
import { queryClient } from "../src/lib/query-client"

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
})
