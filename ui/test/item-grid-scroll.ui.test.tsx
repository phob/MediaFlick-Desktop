import { QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { StrictMode } from "react"
import { createMemoryRouter, Link, RouterProvider, useLocation } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { ItemGrid } from "@/components/ItemGrid"
import { NavigationHistory } from "@/components/NavigationHistory"
import { DetailBackLink } from "@/components/detail/DetailPrimitives"
import { api, PAGE_SIZE, type ItemSummary } from "@/lib/api"
import { testQueryClient } from "./test-query-client"
import { itemSummary } from "./support/fixtures"

// Keep the real paging, router, and virtualizer; artwork/ratings are unrelated.
vi.mock("@/components/MediaCard", () => ({
  MediaCard: ({ item }: { item: ItemSummary }) => <Link to={`/item/${item.id}`}>{item.name}</Link>,
}))
afterEach(() => vi.restoreAllMocks())

test.each(["Movie", "Series"])("%s grid restores a deep item after its catalog cache expires", async (kind) => {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900)
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(900)
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(700)
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value({ top }: ScrollToOptions) {
      const height = Number.parseFloat(this.querySelector(".relative.w-full")?.style.height ?? "0")
      this.scrollTop = Math.min(top ?? 0, Math.max(0, height - 700))
      queueMicrotask(() => { if (this.isConnected) this.dispatchEvent(new Event("scroll")) })
    },
  })
  const items = Array.from({ length: 1200 }, (_, index) => itemSummary({
    id: String(index), kind, name: `${kind} ${index}`,
  }))
  const requests = vi.spyOn(api, "items").mockImplementation(async (query) => ({
    total: items.length, items: items.slice(query.offset ?? 0, (query.offset ?? 0) + PAGE_SIZE),
  }))
  const client = testQueryClient()
  function Page() {
    const { pathname, search } = useLocation()
    return pathname === "/library"
      ? <ItemGrid query={{ kind, sort: new URLSearchParams(search).get("sort") ?? "name" }} />
      : <DetailBackLink to={`/library?kind=${kind}`} label="Back to library" />
  }
  const router = createMemoryRouter([{ path: "*", element: <NavigationHistory><Page /></NavigationHistory> }], {
    initialEntries: [{ pathname: "/library", search: `?kind=${kind}`, key: crypto.randomUUID() }],
  })
  const view = render(<StrictMode><QueryClientProvider client={client}><RouterProvider router={router} /></QueryClientProvider></StrictMode>)
  await screen.findByRole("link", { name: `${kind} 0` })
  const scroller = view.container.querySelector<HTMLElement>(".overflow-y-auto")!
  scroller.scrollTop = 60000
  fireEvent.scroll(scroller)
  const target = await screen.findByRole("link", { name: `${kind} 740` })
  fireEvent.click(target)
  await screen.findByRole("link", { name: "Back to library" })
  client.clear()
  let resolvePage!: (value: { total: number; items: ItemSummary[] }) => void
  const pending = new Promise<{ total: number; items: ItemSummary[] }>((resolve) => { resolvePage = resolve })
  requests.mockImplementationOnce(() => pending)
  fireEvent.click(screen.getByRole("link", { name: "Back to library" }))
  const restored = view.container.querySelector<HTMLElement>(".overflow-y-auto")!
  expect(restored.scrollTop).toBe(0)
  await act(async () => resolvePage({ total: items.length, items: items.slice(0, PAGE_SIZE) }))
  await waitFor(() => expect(restored.scrollTop).toBe(60000))
  await screen.findByRole("link", { name: `${kind} 740` })
  // A changed filter is a fresh list, even though the grid stays mounted.
  await act(() => router.navigate(`/library?kind=${kind}&sort=year`))
  await waitFor(() => expect(restored.scrollTop).toBe(0))
  await act(() => router.navigate(-1))
  await waitFor(() => expect(restored.scrollTop).toBe(60000))
})
