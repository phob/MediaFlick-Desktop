import { QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { useState, type ReactNode } from "react"
import {
  MemoryRouter,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from "react-router-dom"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { SearchBox } from "@/components/AppSidebar"
import { useLiveSearch } from "@/hooks/use-live-search"
import {
  SidebarContext,
  type SidebarContextValue,
} from "@/components/ui/sidebar-context"
import { queryKeys } from "@/lib/query-client"
import Discover from "@/routes/Discover"
import { testQueryClient } from "./test-query-client"

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
  test("waits before searching and replaces history", () => {
    render(
      <TestRouter initialEntries={["/settings", "/"]}>
        <SidebarContext.Provider value={sidebar}>
          <SearchBox />
        </SidebarContext.Provider>
      </TestRouter>,
    )

    const input = screen.getByRole("textbox", { name: "Search the library" })
    fireEvent.change(input, { target: { value: "Matrix" } })
    expect(routeLocation()).toBe("/")
    act(() => vi.advanceTimersByTime(1_000))
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
    const client = testQueryClient()
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
    expect(routeLocation()).toContain("q=old")
    act(() => vi.advanceTimersByTime(1_000))
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

describe("useLiveSearch transition race", () => {
  test("preserves keystrokes typed while an earlier committed search transition is still pending", () => {
    const pendingCommits: Array<() => void> = []
    const committed: string[] = []

    function Harness() {
      const [source, setSource] = useState("")
      const [draft, setDraft] = useLiveSearch(source, (term) => {
        committed.push(term)
        pendingCommits.push(() => setSource(term))
      })
      return <input aria-label="Search" value={draft} onChange={(event) => setDraft(event.target.value)} />
    }

    render(<Harness />)
    const input = screen.getByRole("textbox", { name: "Search" }) as HTMLInputElement

    fireEvent.change(input, { target: { value: "ma" } })
    act(() => vi.advanceTimersByTime(200))
    expect(committed).toEqual(["ma"])

    fireEvent.change(input, { target: { value: "mat" } })
    expect(input.value).toBe("mat")

    act(() => pendingCommits.shift()?.())
    expect(input.value).toBe("mat")

    act(() => vi.advanceTimersByTime(200))
    expect(committed).toEqual(["ma", "mat"])
  })
})
