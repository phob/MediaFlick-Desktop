import { QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { StrictMode } from "react"
import { createMemoryRouter, Outlet, RouterProvider } from "react-router-dom"
import { afterEach, beforeEach, expect, test, vi } from "vitest"
import { RouteScrollViewport } from "@/components/AppShell"
import { NavigationHistory } from "@/components/NavigationHistory"
import { DetailBackLink } from "@/components/detail/DetailPrimitives"
import { api, type SeerrMediaType, type SeerrResult } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import Discover from "@/routes/Discover"
import { testQueryClient } from "./test-query-client"

beforeEach(() => {
  // Model wrapped cards and the real pagination threshold; jsdom has no layout.
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) {
    return 500 + this.querySelectorAll("[data-quick-request-card]").length * 100
  })
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value(this: HTMLElement, { top }: ScrollToOptions) {
      this.scrollTop = Math.min(top ?? 0, Math.max(0, this.scrollHeight - this.clientHeight))
      queueMicrotask(() => { if (this.isConnected) fireEvent.scroll(this) })
    },
  })
  vi.stubGlobal("IntersectionObserver", class {
    node: Element | null = null
    viewport: HTMLElement | null = null
    callback: (entries: { target: Element; isIntersecting: boolean }[]) => void
    constructor(callback: (entries: { target: Element; isIntersecting: boolean }[]) => void) {
      this.callback = callback
    }
    check = () => {
      if (!this.node?.isConnected || !this.viewport) return
      this.callback([{
        target: this.node,
        isIntersecting: this.viewport.scrollTop + this.viewport.clientHeight + 600 >= this.viewport.scrollHeight,
      }])
    }
    observe(node: Element) {
      this.node = node
      this.viewport = node.closest<HTMLElement>(".content-viewport")
      this.viewport?.addEventListener("scroll", this.check)
      queueMicrotask(this.check)
    }
    disconnect() {
      this.viewport?.removeEventListener("scroll", this.check)
      this.node = null
    }
  })
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

function page(mediaType: SeerrMediaType, number: number) {
  return {
    page: number, totalPages: 20, totalResults: 400,
    results: Array.from({ length: 20 }, (_, index): SeerrResult => ({
      mediaType, tmdbId: (number - 1) * 20 + index,
      title: `${mediaType} title ${(number - 1) * 20 + index}`,
      year: 2000, overview: null, posterPath: "/poster.jpg", backdropPath: null,
      voteAverage: null, status: "unknown", status4k: "unknown", libraryItemId: null,
    })),
  }
}

async function browse(url: string, mediaType: SeerrMediaType) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.companion, { compatible: false })
  client.setQueryData(queryKeys.seerrStatus, { capabilities: null })
  client.setQueryData(["seerr", "genres", mediaType], [])
  const discover = vi.spyOn(api.seerr, "discover").mockImplementation(async (_row, _filters, number = 1) => page(mediaType, number))
  const search = vi.spyOn(api.seerr, "search").mockImplementation(async (_term, number = 1) => page(mediaType, number))
  const requests = url.includes("q=") ? search : discover
  const router = createMemoryRouter([{
    element: <NavigationHistory><RouteScrollViewport><Outlet /></RouteScrollViewport></NavigationHistory>,
    children: [
      { path: "/discover", element: <Discover /> },
      { path: "/discover/:type/:id", element: <DetailBackLink to={url} label="Back to discovery" /> },
    ],
  }], { initialEntries: [{ pathname: "/discover", search: `?${url.split("?")[1]}`, key: crypto.randomUUID() }] })
  const view = render(
    <StrictMode><QueryClientProvider client={client}><RouterProvider router={router} /></QueryClientProvider></StrictMode>,
  )
  const viewport = view.container.querySelector<HTMLElement>(".content-viewport")!
  await screen.findByRole("link", { name: `${mediaType} title 0 2000 · ${mediaType === "movie" ? "Movie" : "Series"} View details` })
  for (const top of [1900, 3900]) {
    fireEvent.wheel(viewport)
    viewport.scrollTop = top
    fireEvent.scroll(viewport)
    await waitFor(() => expect(view.container.querySelectorAll("[data-quick-request-card]")).toHaveLength(top === 1900 ? 40 : 60))
  }
  const requestCount = requests.mock.calls.length
  fireEvent.click(screen.getByRole("link", { name: new RegExp(`^${mediaType} title 45 `) }))
  await screen.findByRole("link", { name: "Back to discovery" })
  // Let inactive-query garbage collection run: gcTime: 0 lost all pages here.
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)))
  return { client, router, view, viewport, requests, requestCount }
}

test.each([
  ["/discover?row=movies", "movie"],
  ["/discover?row=tv", "tv"],
  ["/discover?q=space", "movie"],
] as const)("Back to %s restores loaded pages without fetching them again", async (url, mediaType) => {
  const { client, router, view, viewport, requests, requestCount } = await browse(url, mediaType)
  // Aging the snapshot must not cause a background refetch of every page.
  for (const query of client.getQueryCache().findAll({ queryKey: ["seerr", url.includes("q=") ? "search" : "discover"] })) {
    client.setQueryData(query.queryKey, query.state.data, { updatedAt: Date.now() - 60 * 60_000 })
  }
  fireEvent.click(screen.getByRole("link", { name: "Back to discovery" }))
  await waitFor(() => expect(viewport.scrollTop).toBe(3900))
  expect(view.container.querySelectorAll("[data-quick-request-card]")).toHaveLength(60)
  expect(requests).toHaveBeenCalledTimes(requestCount)

  // A different result set starts from page one and the top, with no old cards.
  await act(() => router.navigate(url.includes("q=") ? "/discover?q=ocean" : `${url}&decade=1980`))
  await waitFor(() => expect(view.container.querySelectorAll("[data-quick-request-card]")).toHaveLength(20))
  // StrictMode can cancel and restart the new first-page request.
  const newPages = requests.mock.calls.slice(requestCount).map((args) => args[url.includes("q=") ? 1 : 2])
  expect(newPages.length).toBeGreaterThan(0)
  expect(newPages.every((number) => number === 1)).toBe(true)
  expect(viewport.scrollTop).toBe(0)
  view.unmount()
  client.clear()
})

test("an expired Discovery snapshot does not auto-load pages to reach the saved offset", async () => {
  const { client, view, viewport, requests, requestCount } = await browse("/discover?row=movies", "movie")
  client.removeQueries({ queryKey: ["seerr", "discover"] })
  fireEvent.click(screen.getByRole("link", { name: "Back to discovery" }))
  await waitFor(() => expect(view.container.querySelectorAll("[data-quick-request-card]")).toHaveLength(20))
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)))
  expect(viewport.scrollTop).toBe(0)
  const newPages = requests.mock.calls.slice(requestCount).map((args) => args[2])
  expect(newPages.length).toBeGreaterThan(0)
  expect(newPages.every((number) => number === 1)).toBe(true)
  // Normal user scrolling still loads the next page and cancels restoration.
  fireEvent.wheel(viewport)
  viewport.scrollTop = 1900
  fireEvent.scroll(viewport)
  await waitFor(() => expect(view.container.querySelectorAll("[data-quick-request-card]")).toHaveLength(40))
  expect(viewport.scrollTop).toBe(1900)
  view.unmount()
  client.clear()
})
