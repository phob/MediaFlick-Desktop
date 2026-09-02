import { QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { useState, type ReactNode } from "react"
import { Link, MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, test, vi } from "vitest"
import { LibraryFilters } from "../src/components/LibraryFilters"
import type { ItemGridProps } from "../src/components/ItemGrid"
import {
  libraryKindPath,
  type LibraryFilterState,
} from "../src/lib/library-filters"
import { queryKeys } from "../src/lib/query-client"
import { testQueryClient } from "./test-query-client"

import Library from "../src/routes/Library"

function QueryProbeGrid({ query }: ItemGridProps) {
  return <output data-library-query>{JSON.stringify(query)}</output>
}

const EMPTY: LibraryFilterState = {
  sort: "name",
  genre: "",
  decade: "",
  watched: "",
  favorite: false,
}

function setTouchInput(touch: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: touch && query.includes("pointer: coarse"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

function Providers({ children }: { children: ReactNode }) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.genres, { genres: ["Action", "Drama"] })
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

function FilterHarness({ initial = EMPTY }: { initial?: LibraryFilterState }) {
  const [filters, setFilters] = useState(initial)
  return (
    <>
      <LibraryFilters
        value={filters}
        onChange={(patch) => setFilters((previous) => ({ ...previous, ...patch }))}
        total={123}
      />
      <output data-filter-state>{JSON.stringify(filters)}</output>
    </>
  )
}

function state() {
  return JSON.parse(document.querySelector("[data-filter-state]")?.textContent ?? "{}")
}

beforeEach(() => setTouchInput(false))

describe("consolidated library filters", () => {
  test("touch fallback applies every filter, exposes count/chips, and clears or removes them", () => {
    setTouchInput(true)
    render(<FilterHarness />, { wrapper: Providers })

    expect(screen.getByRole("combobox", { name: "Sort by" })).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Filters" }))

    const dialog = screen.getByRole("dialog", { name: "Filter library" })
    expect(dialog).toBeTruthy()
    expect(screen.getByText(/complete library/i)).toBeTruthy()

    fireEvent.click(screen.getByRole("radio", { name: "Action" }))
    expect(state().genre).toBe("Action")
    fireEvent.click(screen.getByRole("radio", { name: "1990s" }))
    expect(state().decade).toBe("1990")
    fireEvent.click(screen.getByRole("radio", { name: "Unwatched" }))
    expect(state().watched).toBe("false")
    fireEvent.click(screen.getByRole("radio", { name: "In My List" }))
    expect(state().favorite).toBe(true)

    fireEvent.click(screen.getByRole("button", { name: "Done" }))

    expect(screen.getByRole("button", { name: "Filters, 4 active" })).toBeTruthy()
    expect(screen.getByRole("group", { name: "Active filters" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Remove Genre: Action filter" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Remove Released: 1990s filter" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Remove Unwatched filter" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Remove In My List filter" })).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Remove Released: 1990s filter" }))
    expect(state().decade).toBe("")
    expect(screen.getByRole("button", { name: "Filters, 3 active" })).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Clear all" }))
    expect(state()).toEqual(EMPTY)
    expect(screen.queryByRole("group", { name: "Active filters" })).toBeNull()
  })

  test("desktop control uses keyboard-navigable Radix submenus and Escape restores focus", async () => {
    render(<FilterHarness />, { wrapper: Providers })
    const trigger = screen.getByRole("button", { name: "Filters" })

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: "mouse" })
    const genre = await screen.findByRole("menuitem", { name: /Genre/ })
    act(() => genre.focus())
    fireEvent.keyDown(genre, { key: "ArrowRight" })

    const action = await screen.findByRole("menuitemradio", { name: "Action" })
    fireEvent.click(action)
    expect(state().genre).toBe("Action")

    const countedTrigger = screen.getByRole("button", { name: "Filters, 1 active" })
    fireEvent.pointerDown(countedTrigger, { button: 0, ctrlKey: false, pointerType: "mouse" })
    expect(await screen.findByRole("menuitem", { name: /Release decade/ })).toBeTruthy()
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" })

    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    expect(document.activeElement).toBe(countedTrigger)
  })
})

function RouteProbe() {
  const location = useLocation()
  const navigate = useNavigate()
  return (
    <>
      <output data-location>{location.pathname + location.search}</output>
      <button type="button" onClick={() => navigate(-1)}>Back</button>
      <button type="button" onClick={() => navigate(1)}>Forward</button>
      <Link to={libraryKindPath("Series")}>Switch to Series</Link>
    </>
  )
}

function routeLocation() {
  return document.querySelector("[data-location]")?.textContent
}

function routeQuery() {
  return JSON.parse(document.querySelector("[data-library-query]")?.textContent ?? "{}")
}

describe("library filter URL state", () => {
  test("serializes the complete server query and follows back, forward, and kind resets", async () => {
    render(
      <Providers>
        <MemoryRouter
          initialEntries={[
            "/library?kind=Movie&genre=Action&decade=1990&watched=false&favorite=true&sort=year",
          ]}
        >
          <Routes>
            <Route path="/library" element={<Library components={{ ItemGrid: QueryProbeGrid }} />} />
          </Routes>
          <RouteProbe />
        </MemoryRouter>
      </Providers>,
    )

    expect(routeQuery()).toEqual({
      search: "",
      kind: "Movie",
      favorite: true,
      genre: "Action",
      decade: 1990,
      sort: "year",
      watched: "false",
    })

    fireEvent.click(screen.getByRole("button", { name: "Remove Genre: Action filter" }))
    expect(routeLocation()).not.toContain("genre=Action")
    expect(routeQuery().genre).toBe("")

    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    await waitFor(() => expect(routeLocation()).toContain("genre=Action"))
    expect(routeQuery().genre).toBe("Action")

    fireEvent.click(screen.getByRole("button", { name: "Forward" }))
    await waitFor(() => expect(routeLocation()).not.toContain("genre=Action"))
    expect(routeQuery().genre).toBe("")

    fireEvent.click(screen.getByRole("link", { name: "Switch to Series" }))
    await waitFor(() => expect(routeLocation()).toBe("/library?kind=Series"))
    expect(routeQuery()).toEqual({
      search: "",
      kind: "Series",
      genre: "",
      sort: "name",
      watched: "",
    })
    expect(screen.getByRole("heading", { name: "Series" })).toBeTruthy()
  })
})
