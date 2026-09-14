import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { StrictMode } from "react"
import { createMemoryRouter, Link, RouterProvider, useLocation, useNavigate } from "react-router-dom"
import { afterEach, beforeEach, expect, test, vi } from "vitest"
import { NavigationHistory } from "@/components/NavigationHistory"
import { RouteScrollViewport } from "@/components/AppShell"
import { DetailBackLink } from "@/components/detail/DetailPrimitives"

afterEach(() => vi.restoreAllMocks())
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) {
    return Number(this.querySelector("[data-height]")?.getAttribute("data-height") ?? 10000)
  })
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value({ top }: ScrollToOptions) {
      const height = Number(this.querySelector("[data-height]")?.getAttribute("data-height") ?? 10000)
      this.scrollTop = Math.min(top ?? 0, height)
    },
  })
})

function Page({ height }: { height: number }) {
  const location = useLocation()
  const navigate = useNavigate()
  return <>
    <output>{location.pathname}{location.search}</output>
    <div key={location.key} data-height={height}>
      <Link to="/item/star-trek">Star Trek</Link>
      <Link to="/item/episode">Episode</Link>
      <Link to="/library?kind=Series">Series</Link>
      <button onClick={() => void navigate("?season=2", { replace: true })}>Season</button>
      <DetailBackLink to="/library?kind=Movie&sort=name" label="Back to library" />
    </div>
  </>
}

function setup(initial = "/library?kind=Movie&sort=name", initialHeight = 10000) {
  let height = initialHeight
  function Surface() {
    useLocation()
    return <NavigationHistory><RouteScrollViewport><Page height={height} /></RouteScrollViewport></NavigationHistory>
  }
  const router = createMemoryRouter([{ path: "*", element: <Surface /> }], {
    initialEntries: [{ pathname: initial.split("?")[0], search: initial.includes("?") ? `?${initial.split("?")[1]}` : "", key: crypto.randomUUID() }],
  })
  const view = render(<StrictMode><RouterProvider router={router} /></StrictMode>)
  const viewport = view.container.querySelector<HTMLElement>(".content-viewport")!
  return { router, viewport, setHeight: (value: number) => { height = value } }
}

test("visible Back restores the original entry and filters, even through nested details and a replacement", async () => {
  const { router, viewport } = setup()
  const originalKey = router.state.location.key
  viewport.scrollTop = 7200
  fireEvent.scroll(viewport)
  fireEvent.click(screen.getByRole("link", { name: "Star Trek" }))
  await waitFor(() => expect(viewport.scrollTop).toBe(0))
  fireEvent.click(screen.getByRole("link", { name: "Episode" }))
  fireEvent.click(screen.getByRole("button", { name: "Season" }))
  fireEvent.click(screen.getByRole("link", { name: "Back to library" }))
  await waitFor(() => expect(router.state.location.key).toBe(originalKey))
  expect(viewport.scrollTop).toBe(7200)
  expect(router.state.location.search).toBe("?kind=Movie&sort=name")
})

test("a direct detail visit has a working fallback Back link", async () => {
  const { router } = setup("/item/direct")
  fireEvent.click(screen.getByRole("link", { name: "Back to library" }))
  await waitFor(() => expect(router.state.location.pathname).toBe("/library"))
})

test("mouse and keyboard shortcuts move exactly one entry and restore both directions", async () => {
  const { router, viewport } = setup()
  viewport.scrollTop = 8100
  fireEvent.scroll(viewport)
  fireEvent.click(screen.getByRole("link", { name: "Star Trek" }))
  await waitFor(() => expect(router.state.location.pathname).toBe("/item/star-trek"))
  viewport.scrollTop = 420
  fireEvent.scroll(viewport)
  fireEvent.mouseDown(window, { button: 3 })
  fireEvent.mouseUp(window, { button: 3 })
  fireEvent(window, new MouseEvent("auxclick", { button: 3, cancelable: true }))
  await waitFor(() => expect(router.state.location.pathname).toBe("/library"))
  expect(viewport.scrollTop).toBe(8100)
  fireEvent.keyDown(window, { key: "ArrowRight", altKey: true })
  await waitFor(() => expect(router.state.location.pathname).toBe("/item/star-trek"))
  expect(viewport.scrollTop).toBe(420)
  fireEvent.keyDown(window, { key: "ArrowLeft", altKey: true })
  await waitFor(() => expect(viewport.scrollTop).toBe(8100))
  fireEvent.mouseUp(window, { button: 4 })
  await waitFor(() => expect(viewport.scrollTop).toBe(420))
})

test("restoration waits for late content and stops when the user scrolls", async () => {
  const { router, viewport, setHeight } = setup()
  viewport.scrollTop = 6000
  fireEvent.scroll(viewport)
  fireEvent.click(screen.getByRole("link", { name: "Star Trek" }))
  setHeight(100)
  await act(() => router.navigate(-1))
  expect(viewport.scrollTop).toBe(0)
  // Waiting at the top must not overwrite the original target or chase the
  // growing bottom (which would trigger an infinite list's pagination).
  fireEvent.scroll(viewport)
  viewport.querySelector("[data-height]")!.setAttribute("data-height", "3000")
  await act(async () => {})
  expect(viewport.scrollTop).toBe(0)
  viewport.querySelector("[data-height]")!.setAttribute("data-height", "10000")
  await waitFor(() => expect(viewport.scrollTop).toBe(6000))
  fireEvent.click(screen.getByRole("link", { name: "Star Trek" }))
  await act(() => router.navigate(-1))
  expect(viewport.scrollTop).toBe(0)
  fireEvent.wheel(viewport)
  viewport.scrollTop = 40
  fireEvent.scroll(viewport)
  viewport.querySelector("[data-height]")!.setAttribute("data-height", "10000")
  await act(async () => {})
  expect(viewport.scrollTop).toBe(40)
})

test("new visits to the same URL start at the top", async () => {
  const { router, viewport } = setup()
  viewport.scrollTop = 5000
  fireEvent.scroll(viewport)
  fireEvent.click(screen.getByRole("link", { name: "Star Trek" }))
  await act(() => router.navigate("/library?kind=Movie&sort=name"))
  expect(viewport.scrollTop).toBe(0)
})
