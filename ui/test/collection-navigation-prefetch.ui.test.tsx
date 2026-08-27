import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { AppSidebar } from "../src/components/AppSidebar"
import { SidebarProvider } from "../src/components/ui/sidebar"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe("collection navigation prefetch", () => {
  test("warms both MediaFlick collection menus on navigation intent", async () => {
    vi.stubGlobal("matchMedia", vi.fn(() => ({
      matches: false,
      media: "",
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    })))
    const franchises = vi.spyOn(api.api.collections, "franchises").mockResolvedValue({
      status: "ready",
      franchises: [],
    })
    const mine = vi.spyOn(api.api.collections, "mine").mockResolvedValue({ profiles: [] })
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Infinity } },
    })
    const account = "https://jellyfin.example:user-1"
    client.setQueryData(queryKeys.status, {
      authenticated: true,
      serverUrl: "https://jellyfin.example",
      userId: "user-1",
      userName: "Neo",
    })
    client.setQueryData(queryKeys.companion, { compatible: false })
    client.setQueryData(queryKeys.collectionSettings(account), {
      effectiveMode: "mediaFlick",
      mediaFlickAvailable: true,
      modeSelection: "mediaFlick",
      franchises: { includeUnreleased: false },
      readiness: { tmdb: true, mdblist: false },
      recovery: null,
      access: { readOnly: false },
    })

    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <SidebarProvider>
            <AppSidebar />
          </SidebarProvider>
        </MemoryRouter>
      </QueryClientProvider>,
    )

    fireEvent.pointerEnter(screen.getByRole("link", { name: "Movie Franchises" }))
    fireEvent.focus(screen.getByRole("link", { name: "My Collections" }))
    await waitFor(() => {
      expect(franchises).toHaveBeenCalledOnce()
      expect(mine).toHaveBeenCalledOnce()
    })
  })
})
