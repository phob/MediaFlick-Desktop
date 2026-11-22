import { render, screen } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { api, type CompanionStatus } from "@/lib/api"
import Settings from "@/routes/Settings"
import { queryKeys } from "@/lib/query-client"
import { appStatus } from "./support/fixtures"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

afterEach(() => vi.restoreAllMocks())

function missingCompanion(serverTooOld: CompanionStatus["serverTooOld"]): CompanionStatus {
  return {
    available: false,
    compatible: false,
    checked: true,
    info: null,
    error: null,
    supportedApi: { min: 1, max: 1 },
    serverTooOld,
  }
}

function renderCompanion(companion: CompanionStatus) {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true, userId: "account-1" }))
  vi.spyOn(api.companion, "info").mockResolvedValue(companion)
  render(<TestProviders client={client} initialEntries={["/settings/integrations/companion"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)
}

test("a server older than Jellyfin 12 says the Companion needs Jellyfin 12.0.0", async () => {
  renderCompanion(missingCompanion({ version: "10.11.11", minimum: "12.0.0" }))
  expect(await screen.findByText(
    "MediaFlick Companion needs Jellyfin 12.0.0 or newer. This server runs Jellyfin 10.11.11.",
  )).toBeTruthy()
  expect(screen.queryByText(/was not found/)).toBeNull()
})

test("a supported server without the plugin keeps the not-found text", async () => {
  renderCompanion(missingCompanion(null))
  expect(await screen.findByText("MediaFlick Companion was not found on this Jellyfin server.")).toBeTruthy()
})
