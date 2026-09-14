import { fireEvent, render, screen } from "@testing-library/react"
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { RouteScrollViewport } from "@/components/AppShell"

function First() {
  const navigate = useNavigate()
  return <button onClick={() => navigate("/second")}>Second</button>
}

function Second() {
  const navigate = useNavigate()
  return <button onClick={() => navigate(-1)}>Back</button>
}

describe("AppShell route scrolling", () => {
  afterEach(() => vi.restoreAllMocks())
  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(10000)
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value({ top }: ScrollToOptions) {
        this.scrollTop = top ?? 0
      },
    })
  })

  test("new routes start at the top and browser Back restores the old route", () => {
    const view = render(
      <MemoryRouter initialEntries={["/first"]}>
        <RouteScrollViewport>
          <Routes>
            <Route path="/first" element={<First />} />
            <Route path="/second" element={<Second />} />
          </Routes>
        </RouteScrollViewport>
      </MemoryRouter>,
    )
    const viewport = view.container.querySelector<HTMLElement>(".content-viewport")
    if (!viewport) throw new Error("Expected the route scroll viewport")
    viewport.scrollTop = 180
    fireEvent.scroll(viewport)

    fireEvent.click(screen.getByRole("button", { name: "Second" }))
    expect(viewport.scrollTop).toBe(0)

    viewport.scrollTop = 90
    fireEvent.scroll(viewport)
    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    expect(viewport.scrollTop).toBe(180)
  })
})
