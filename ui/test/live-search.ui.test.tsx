import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { type ReactNode } from "react"
import {
  MemoryRouter,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { SearchBox } from "@/components/AppSidebar"
import {
  SidebarContext,
  type SidebarContextValue,
} from "@/components/ui/sidebar-context"
import { queryKeys } from "@/lib/query-client"
import Discover from "@/routes/Discover"

const sidebar: SidebarContextValue = {
  state: "expanded",
  isMobile: false,
}

function LocationProbe() {
  const location = useLocation()
  const navigate = useNavigate()
  return (
    <>
      <output data-location>{location.pathname + location.search}</output>
      <button type="button" onClick={() => navigate(-1)}>Back</button>
    </>
  )
}

function routeLocation() {
  return document.querySelector("[data-location]")?.textContent
}

function TestRouter({
  children,
  initialEntries,
  initialIndex = initialEntries.length - 1,
}: {
  children: ReactNode
  initialEntries: string[]
  initialIndex?: number
}) {
  return (
    <MemoryRouter initialEntries={initialEntries} initialIndex={initialIndex}>
      {children}
      <LocationProbe />
    </MemoryRouter>
  )
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe("sidebar live search", () => {
  test("waits 200 ms, starts at two characters, and replaces history", () => {
    render(
      <TestRouter initialEntries={["/settings", "/"]}>
        <SidebarContext.Provider value={sidebar}>
          <SearchBox />
        </SidebarContext.Provider>
      </TestRouter>,
    )

    const input = screen.getByRole("textbox", { name: "Search the library" })
    fireEvent.change(input, { target: { value: "m" } })
    expect(routeLocation()).toBe("/")

    fireEvent.change(input, { target: { value: "ma" } })
    act(() => vi.advanceTimersByTime(100))
    fireEvent.change(input, { target: { value: "Matrix" } })
    act(() => vi.advanceTimersByTime(199))
    expect(routeLocation()).toBe("/")
    act(() => vi.advanceTimersByTime(1))
    expect(routeLocation()).toBe("/library?search=Matrix")

    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    expect(routeLocation()).toBe("/settings")
  })

  test("Enter searches immediately, globally, and an incomplete draft clears results", () => {
    render(
      <TestRouter initialEntries={["/library?kind=Movie&favorite=true"]}>
        <SidebarContext.Provider value={sidebar}>
          <SearchBox />
        </SidebarContext.Provider>
      </TestRouter>,
    )

    const input = screen.getByRole("textbox", { name: "Search the library" })
    fireEvent.change(input, { target: { value: "Alien" } })
    const form = input.parentElement
    if (!form) throw new Error("Sidebar search form is missing")
    fireEvent.submit(form)
    expect(routeLocation()).toBe("/library?search=Alien")

    fireEvent.change(input, { target: { value: "a" } })
    expect(routeLocation()).toBe("/library")
    expect(input).toHaveProperty("value", "a")
  })
})

describe("Discover live search", () => {
  test("debounces query changes while preserving discovery state", () => {
    const client = new QueryClient({
      defaultOptions: { queries: { gcTime: Infinity, retry: false } },
    })
    client.setQueryData(queryKeys.companion, {
      available: false,
      compatible: false,
      info: null,
    })
    client.setQueryData(queryKeys.seerrSearchInfinite("old"), {
      pages: [{ page: 1, totalPages: 1, totalResults: 0, results: [] }],
      pageParams: [1],
    })
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      page: 1,
      totalPages: 1,
      totalResults: 0,
      results: [],
    }), { headers: { "content-type": "application/json" } })))

    render(
      <QueryClientProvider client={client}>
        <TestRouter
          initialEntries={[
            "/settings",
            "/discover?row=movies&library=outside&genre=18&q=old",
          ]}
        >
          <Routes>
            <Route path="/discover" element={<Discover />} />
            <Route path="*" element={<div />} />
          </Routes>
        </TestRouter>
      </QueryClientProvider>,
    )

    const input = screen.getByRole("textbox", { name: "Search Seerr" })
    fireEvent.change(input, { target: { value: "Matrix" } })
    act(() => vi.advanceTimersByTime(199))
    expect(routeLocation()).toContain("q=old")
    act(() => vi.advanceTimersByTime(1))
    expect(routeLocation()).toBe(
      "/discover?row=movies&library=outside&genre=18&q=Matrix",
    )

    fireEvent.change(input, { target: { value: "m" } })
    expect(routeLocation()).toBe("/discover?row=movies&library=outside&genre=18")
    expect(input).toHaveProperty("value", "m")

    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    expect(routeLocation()).toBe("/settings")
    client.clear()
  })
})
