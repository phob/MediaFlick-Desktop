import { fireEvent, render, screen } from "@testing-library/react"
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, test } from "vitest"
import { RouteScrollViewport } from "@/components/AppShell"
import { sidebarShouldBeOpen, sidebarShouldOverlayContent } from "@/lib/sidebar-state"

function First() {
  const navigate = useNavigate()
  return <button onClick={() => navigate("/second")}>Second</button>
}

function Second() {
  const navigate = useNavigate()
  return <button onClick={() => navigate(-1)}>Back</button>
}

describe("AppShell route scrolling", () => {
  beforeEach(() => {
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

    fireEvent.click(screen.getByRole("button", { name: "Second" }))
    expect(viewport.scrollTop).toBe(0)

    viewport.scrollTop = 90
    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    expect(viewport.scrollTop).toBe(180)
  })
})

describe("AppShell sidebar state", () => {
  test("stays open on Home and opens other routes only while hovered", () => {
    expect(sidebarShouldBeOpen("/", false)).toBe(true)
    expect(sidebarShouldBeOpen("/", true)).toBe(true)
    expect(sidebarShouldBeOpen("/library", false)).toBe(false)
    expect(sidebarShouldBeOpen("/library", true)).toBe(true)
    expect(sidebarShouldBeOpen("/item/123", false)).toBe(false)
  })

  test("overlays content outside Home", () => {
    expect(sidebarShouldOverlayContent("/")).toBe(false)
    expect(sidebarShouldOverlayContent("/library")).toBe(true)
    expect(sidebarShouldOverlayContent("/item/123")).toBe(true)
  })
})
