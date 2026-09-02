import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import type { LetterboxdProfile } from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import Settings from "../src/routes/Settings"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

const profile: LetterboxdProfile = {
  id: "profile-1",
  provider: "letterboxd",
  profileKey: "neo",
  displayName: "Neo",
  canonicalUrl: "https://letterboxd.com/neo/",
  enabled: false,
  verificationStatus: "verified",
  createdAt: 1,
  lastCheckedAt: 1,
}

afterEach(() => vi.restoreAllMocks())

test("Letterboxd enablement waits for Save and supports Reset and Discard", async () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, { authenticated: true })
  client.setQueryData(["letterboxd", "profiles"], { profiles: [profile] })
  vi.spyOn(api.api.letterboxd, "profiles").mockResolvedValue({ profiles: [{ ...profile, enabled: true }] })
  const setEnabled = vi.spyOn(api.api.letterboxd, "setEnabled").mockResolvedValue({
    profile: { ...profile, enabled: true },
  })

  render(
    <TestProviders client={client} initialEntries={["/settings/integrations/letterboxd"]}>
        <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  const enabled = screen.getByRole("switch", { name: "Enable Neo" })
  fireEvent.click(enabled)
  expect(setEnabled).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  expect(enabled.getAttribute("aria-checked")).toBe("false")

  fireEvent.click(enabled)
  fireEvent.click(screen.getByRole("button", { name: "Reset" }))
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(setEnabled).toHaveBeenCalledWith(profile.id, true))
})
