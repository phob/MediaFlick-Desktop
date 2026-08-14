import { QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import { afterEach, describe, expect, test } from "vitest"
import { LibrarySyncProgress } from "../src/components/AppSidebar"
import type { Status, SyncProgress } from "../src/lib/api"
import { queryClient, queryKeys } from "../src/lib/query-client"

const catalog = {
  complete: true,
  ready: true,
  processed: 120,
  total: 120,
  initial: false,
}

function status(progress: SyncProgress): Status {
  return { authenticated: true, libraryReady: true, syncProgress: progress }
}

function renderProgress(progress: SyncProgress) {
  queryClient.setQueryData(queryKeys.status, status(progress))
  return render(
    <QueryClientProvider client={queryClient}>
      <LibrarySyncProgress />
    </QueryClientProvider>,
  )
}

describe("sidebar synchronization progress", () => {
  afterEach(() => queryClient.removeQueries({ queryKey: queryKeys.status }))

  test("shows one compact determinate catalog update above settings", () => {
    renderProgress({
      active: true,
      phase: "catalog",
      catalog: { ...catalog, complete: false, processed: 40 },
      error: null,
      retryAt: null,
    })

    expect(screen.getByRole("status").textContent).toContain("Loading library")
    expect(screen.getByText("40 of 120")).toBeTruthy()
    const bar = screen.getByRole("progressbar", { name: "Loading library" })
    expect(bar.getAttribute("aria-valuenow")).toBe("40")
    expect(bar.getAttribute("aria-valuemax")).toBe("120")
  })

  test("explains a nonmodal retry and disappears once work settles", () => {
    const { rerender } = renderProgress({
      active: true,
      phase: "retrying",
      catalog,
      error: "Jellyfin rate limited requests",
      retryAt: 1_900_000_000,
    })

    const retry = screen.getByRole("status")
    expect(retry.textContent).toContain("Synchronization paused")
    expect(retry.textContent).toContain("Retry scheduled")
    expect(retry.getAttribute("title")).toBe("Jellyfin rate limited requests")

    queryClient.setQueryData(
      queryKeys.status,
      status({
        active: false,
        phase: "complete",
        catalog,
        error: null,
        retryAt: null,
      }),
    )
    rerender(
      <QueryClientProvider client={queryClient}>
        <LibrarySyncProgress />
      </QueryClientProvider>,
    )
    expect(screen.queryByRole("status")).toBeNull()
  })
})
