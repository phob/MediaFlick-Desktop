import { render, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, test, vi } from "vitest"
import { CollectionTitleGrid } from "../src/components/CollectionTitleGrid"

afterEach(() => vi.restoreAllMocks())

describe("collection title grid", () => {
  test("mounts only a window of a large collection", async () => {
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900)
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(900)
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) {
      return this.classList.contains("content-viewport") ? 700 : 306
    })

    const scroller = document.createElement("div")
    scroller.className = "content-viewport"
    document.body.append(scroller)
    const items = Array.from({ length: 200 }, (_, id) => ({ id }))

    const view = render(
      <CollectionTitleGrid
        items={items}
        itemKey={(item) => String(item.id)}
        renderItem={(item) => <span data-testid="collection-card">{item.id}</span>}
      />,
      { container: scroller },
    )

    await waitFor(() => {
      const mounted = view.getAllByTestId("collection-card").length
      expect(mounted).toBeGreaterThan(0)
      expect(mounted).toBeLessThan(50)
    })
    scroller.remove()
  })
})
