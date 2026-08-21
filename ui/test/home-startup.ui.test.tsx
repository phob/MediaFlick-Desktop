import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import { useContext } from "react"
import { describe, expect, test } from "vitest"
import { AppProviders } from "@/components/AppProviders"
import { RatingsContext } from "@/lib/rating-context"
import { queryKeys } from "@/lib/query-client"
import { startupScreenReady } from "@/lib/startup"

function RatingsProbe() {
  const ratings = useContext(RatingsContext)
  return <output data-testid="ratings-context">{ratings ? "available" : "missing"}</output>
}

describe("home startup cover", () => {
  test("keeps shell content inside the real rating and technical providers", () => {
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } })
    client.setQueryData(queryKeys.settings, {
      appearance: { showMediaInfo: false, ratingSources: [] },
    })
    client.setQueryData(queryKeys.ratingsStatus, {
      selectionEnabled: false,
      effectiveOrigin: "none",
      sources: [],
    })

    const view = render(
      <QueryClientProvider client={client}>
        <AppProviders>
          <RatingsProbe />
        </AppProviders>
      </QueryClientProvider>,
    )

    expect(screen.getByTestId("ratings-context").textContent).toBe("available")
    view.unmount()
    client.clear()
  })

  test("stays up until both SQLite-backed home queries settle", () => {
    const readiness = {
      statusPending: false,
      settingsPending: false,
      waitingForLibrary: false,
      showingSettings: false,
      initialHomeEnabled: true,
      homePending: true,
      billboardPending: true,
    }

    expect(startupScreenReady(readiness)).toBe(false)
    expect(startupScreenReady({ ...readiness, homePending: false })).toBe(false)
    expect(startupScreenReady({ ...readiness, homePending: false, billboardPending: false })).toBe(true)
    expect(startupScreenReady({ ...readiness, settingsPending: true })).toBe(false)
  })

  test("keeps library startup gated while settings remains directly available", () => {
    const readiness = {
      statusPending: false,
      settingsPending: false,
      waitingForLibrary: true,
      showingSettings: false,
      initialHomeEnabled: false,
      homePending: false,
      billboardPending: false,
    }

    expect(startupScreenReady(readiness)).toBe(false)
    expect(startupScreenReady({ ...readiness, showingSettings: true })).toBe(true)
  })
})
