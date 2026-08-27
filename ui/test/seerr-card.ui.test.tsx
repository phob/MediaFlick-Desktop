import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { SeerrCard } from "../src/components/seerr/SeerrCard"
import type {
  SeerrCapabilities,
  SeerrResult,
  SeerrStatusInfo,
} from "../src/lib/api"
import { canQuickRequest } from "../src/lib/seerr-request"
import { queryKeys } from "../src/lib/query-client"

const capabilities: SeerrCapabilities = {
  movie: { request: true, autoApprove: false },
  tv: { request: true, autoApprove: false },
  movie4k: { request: false, autoApprove: false },
  tv4k: { request: false, autoApprove: false },
  advancedRequest: false,
}

const movie: SeerrResult = {
  mediaType: "movie",
  tmdbId: 603,
  title: "The Matrix",
  year: 1999,
  overview: null,
  posterPath: null,
  backdropPath: null,
  voteAverage: 8.2,
  status: "unknown",
  status4k: "unknown",
  libraryItemId: null,
}

const status: SeerrStatusInfo = {
  linked: true,
  mapped: true,
  instance: {
    movie4kEnabled: false,
    series4kEnabled: false,
    partialRequestsEnabled: true,
  },
  user: { id: 1, name: "Neo", avatar: null, jellyfinUserId: "neo" },
  capabilities,
  quota: {
    movie: { days: null, limit: null, used: 0, remaining: null, restricted: false },
    tv: { days: null, limit: null, used: 0, remaining: null, restricted: false },
  },
}

function LocationProbe() {
  const location = useLocation()
  return <output data-location>{location.pathname + location.search}</output>
}

function Providers({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  client.setQueryData(queryKeys.seerrStatus, status)

  return (
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/discover?row=movies"]}>
        {children}
        <LocationProbe />
      </MemoryRouter>
    </QueryClientProvider>
  )
}

function renderCard(
  result: SeerrResult = movie,
  allowed: SeerrCapabilities | null = capabilities,
) {
  return render(<SeerrCard result={result} capabilities={allowed} />, {
    wrapper: Providers,
  })
}

function location() {
  return document.querySelector("[data-location]")?.textContent
}

afterEach(() => vi.restoreAllMocks())

describe("discovery quick request", () => {
  test("reveals the overlay on pointer hover and keyboard focus", () => {
    renderCard()
    const action = screen.getByRole("button", { name: "Request The Matrix" })
    const card = action.closest("[data-quick-request-card]")!
    const detail = screen.getByRole("link")

    expect(action.className).toContain("opacity-0")
    expect(card.getAttribute("data-quick-request-visible")).toBeNull()

    fireEvent.pointerEnter(card)
    expect(action.className).toContain("opacity-100")
    expect(card.getAttribute("data-quick-request-visible")).toBe("true")

    fireEvent.pointerLeave(card)
    expect(action.className).toContain("opacity-0")

    act(() => detail.focus())
    expect(action.className).toContain("opacity-100")
    act(() => action.focus())
    expect(document.activeElement).toBe(action)
  })

  test("intercepts the card click and opens the existing request dialog", () => {
    renderCard()
    const action = screen.getByRole("button", { name: "Request The Matrix" })
    // React delegates clicks at the render root. Observe one level above it so
    // stopPropagation has a real boundary to cross (listeners on the same root
    // node would still run, just like two native listeners on any one node).
    const outside = action.closest("[data-quick-request-card]")!.parentElement!.parentElement!
    const bubbled = vi.fn()
    outside.addEventListener("click", bubbled)
    const click = new MouseEvent("click", { bubbles: true, cancelable: true })

    act(() => action.dispatchEvent(click))

    expect(click.defaultPrevented).toBe(true)
    expect(bubbled).not.toHaveBeenCalled()
    expect(location()).toBe("/discover?row=movies")
    expect(screen.getByRole("dialog")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "Request The Matrix (1999)" })).toBeTruthy()
  })

  test("keeps the rest of the card linked to discovery details", () => {
    renderCard()

    fireEvent.click(screen.getByRole("link"))

    expect(location()).toBe("/discover/movie/603?row=movies")
    expect(screen.queryByRole("dialog")).toBeNull()
  })

  test("closing and submitting never activate the detail route", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          id: 12,
          status: "approved",
          mediaType: "movie",
          tmdbId: 603,
          is4k: false,
          createdAt: null,
          updatedAt: null,
          mediaStatus: "pending",
          seasons: [],
          libraryItemId: null,
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    )
    renderCard()

    fireEvent.click(screen.getByRole("button", { name: "Request The Matrix" }))
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(screen.queryByRole("dialog")).toBeNull()
    expect(location()).toBe("/discover?row=movies")

    fireEvent.click(screen.getByRole("button", { name: "Request The Matrix" }))
    fireEvent.click(screen.getByRole("button", { name: "Request" }))
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/seerr/request",
      expect.objectContaining({ method: "POST" }),
    )
    expect(location()).toBe("/discover?row=movies")
  })

  test("shows only statuses with a real regular, partial-series, or 4K path", () => {
    const noFourK = capabilities
    const withFourK = {
      ...capabilities,
      movie4k: { request: true, autoApprove: false },
      tv4k: { request: true, autoApprove: false },
    }
    const fullyAvailable = { ...movie, status: "available", status4k: "available" } as const
    const fullyRequested = { ...movie, status: "pending", status4k: "processing" } as const
    const ownedWithFourKPath = {
      ...movie,
      status: "available",
      status4k: "unknown",
      libraryItemId: "local-movie",
    } as const
    const partialSeries = {
      ...movie,
      mediaType: "tv",
      status: "partial",
      status4k: "available",
      libraryItemId: "local-series",
    } as const
    const ownedButUnknownToSeerr = {
      ...movie,
      status: "unknown",
      status4k: "available",
      libraryItemId: "local-movie",
    } as const

    expect(canQuickRequest(fullyAvailable, withFourK)).toBe(false)
    expect(canQuickRequest(fullyRequested, withFourK)).toBe(false)
    expect(canQuickRequest(ownedButUnknownToSeerr, noFourK)).toBe(false)
    expect(canQuickRequest(ownedWithFourKPath, noFourK)).toBe(false)
    expect(canQuickRequest(ownedWithFourKPath, withFourK)).toBe(true)
    expect(canQuickRequest(partialSeries, capabilities)).toBe(true)

    const { rerender } = renderCard(fullyAvailable, withFourK)
    expect(screen.queryByRole("button", { name: "Request The Matrix" })).toBeNull()

    rerender(<SeerrCard result={partialSeries} capabilities={capabilities} />)
    expect(screen.getByRole("button", { name: "Request The Matrix" })).toBeTruthy()
  })
})
