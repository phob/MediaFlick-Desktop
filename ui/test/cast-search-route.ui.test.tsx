import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router-dom"
import { describe, expect, test, vi } from "vitest"
import type { ItemQuery } from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"

vi.mock("@/components/ItemGrid", () => ({
  ItemGrid: ({ query, footer }: { query: ItemQuery; footer?: ReactNode }) => (
    <>
      <output data-library-query>{JSON.stringify(query)}</output>
      {footer}
    </>
  ),
}))

vi.mock("@/components/seerr/CastDiscover", () => ({
  CastDiscover: ({ personName, jellyfinId, tmdbId, resolving }: {
    personName: string
    jellyfinId: string | null
    tmdbId: number | null
    resolving?: boolean
  }) => (
    <output data-cast-discover>
      {JSON.stringify({ personName, jellyfinId, tmdbId, resolving })}
    </output>
  ),
}))

import Library from "../src/routes/Library"

function NavigationProbe() {
  const location = useLocation()
  const navigate = useNavigate()
  return (
    <>
      <output data-location>{location.pathname + location.search}</output>
      <button type="button" onClick={() => navigate(-1)}>Back</button>
      <button type="button" onClick={() => navigate(1)}>Forward</button>
    </>
  )
}

function routeQuery() {
  return JSON.parse(document.querySelector("[data-library-query]")?.textContent ?? "{}")
}

function location() {
  return document.querySelector("[data-location]")?.textContent ?? ""
}

function Providers({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(queryKeys.genres, { genres: [] })
  client.setQueryData(queryKeys.personResolution("jf-keanu", null, "Keanu Reeves"), {
    person: {
      jellyfinId: "jf-keanu",
      tmdbId: 6384,
      name: "Keanu Reeves",
      imageTag: null,
    },
    candidates: [],
    ambiguous: false,
  })
  client.setQueryData(queryKeys.personResolution("jf-keanu", 6384, "Keanu Reeves"), {
    person: {
      jellyfinId: "jf-keanu",
      tmdbId: 6384,
      name: "Keanu Reeves",
      imageTag: null,
    },
    candidates: [],
    ambiguous: false,
  })
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

describe("person-mode Search route", () => {
  test("deep links restore exact-person server filtering while back/forward preserves normal search", async () => {
    render(
      <Providers>
        <MemoryRouter
          initialEntries={[
            "/library?search=Matrix&sort=rating",
            "/library?mode=person&personId=jf-keanu&personName=Keanu+Reeves",
          ]}
          initialIndex={1}
        >
          <Routes><Route path="/library" element={<Library />} /></Routes>
          <NavigationProbe />
        </MemoryRouter>
      </Providers>,
    )

    expect(screen.getByText("On your Jellyfin server")).toBeTruthy()
    expect(routeQuery()).toEqual({ personId: "jf-keanu" })
    await waitFor(() => expect(location()).toContain("tmdbPersonId=6384"))
    expect(JSON.parse(document.querySelector("[data-cast-discover]")?.textContent ?? "{}")).toMatchObject({
      personName: "Keanu Reeves",
      jellyfinId: "jf-keanu",
      tmdbId: 6384,
    })

    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    await waitFor(() => expect(location()).toContain("search=Matrix"))
    expect(routeQuery()).toMatchObject({ search: "Matrix", sort: "rating" })
    expect(screen.queryByText("On your Jellyfin server")).toBeNull()

    fireEvent.click(screen.getByRole("button", { name: "Forward" }))
    await waitFor(() => expect(routeQuery()).toEqual({ personId: "jf-keanu" }))
    expect(screen.getByText("On your Jellyfin server")).toBeTruthy()
  })
})
