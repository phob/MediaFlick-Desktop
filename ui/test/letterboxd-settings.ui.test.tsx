import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes, useLocation, useNavigate } from "react-router-dom"
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

function NavigationProbe() {
  const location = useLocation()
  const navigate = useNavigate()
  return <><output aria-label="Current page">{location.pathname}</output><button onClick={() => void navigate(-1)}>Back</button></>
}

function renderProfiles() {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, { authenticated: true, userId: "account-1" })
  client.setQueryData(["letterboxd", "profiles"], { profiles: [profile] })
  vi.spyOn(api.api.letterboxd, "profiles").mockImplementation(async () => ({
    profiles: client.getQueryData<{ profiles: LetterboxdProfile[] }>(["letterboxd", "profiles"])?.profiles ?? [],
  }))
  render(<TestProviders client={client} initialEntries={["/settings/client/player", "/settings/integrations/letterboxd"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    <NavigationProbe />
  </TestProviders>)
  return client
}

test("Reset is available before editing and Discard restores saved enablement", () => {
  renderProfiles()
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(screen.getByRole("button", { name: "Reset" }))
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("true")
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("false")
})

test.each(["Player", "Back"])("unsaved settings guard %s navigation and preserve edits when dismissed", async (navigation) => {
  renderProfiles()
  fireEvent.click(screen.getByRole("switch", { name: "Enable Neo" }))
  const leave = () => fireEvent.click(screen.getByRole(navigation === "Back" ? "button" : "link", { name: navigation }))
  leave()
  expect(await screen.findByRole("dialog", { name: "Leave without saving?" })).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }))
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("true")
  leave()
  fireEvent.click(await screen.findByRole("button", { name: "Discard and leave" }))
  await waitFor(() => expect(screen.getByLabelText("Current page").textContent).toBe("/settings/client/player"))
  fireEvent.click(screen.getByRole("link", { name: "Letterboxd" }))
  expect(await screen.findByRole("switch", { name: "Enable Neo" })).toBeTruthy()
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("false")
})

test("refreshed profiles retain an unsaved toggle and expose newly connected profiles", async () => {
  const client = renderProfiles()
  fireEvent.click(screen.getByRole("switch", { name: "Enable Neo" }))
  act(() => client.setQueryData(["letterboxd", "profiles"], { profiles: [profile, { ...profile, id: "second", displayName: "Trinity", enabled: true }] }))
  expect(await screen.findByRole("switch", { name: "Enable Trinity" })).toBeTruthy()
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("true")
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("false")
  expect(screen.getByRole("switch", { name: "Enable Trinity" }).getAttribute("aria-checked")).toBe("true")
})

test("profile additions and removals can be undone or discarded without writes", () => {
  const add = vi.spyOn(api.api.letterboxd, "add")
  const remove = vi.spyOn(api.api.letterboxd, "remove")
  renderProfiles()
  fireEvent.change(screen.getByRole("textbox", { name: "Letterboxd username or profile URL" }), { target: { value: "trinity" } })
  fireEvent.click(screen.getByRole("button", { name: "Add profile" }))
  fireEvent.click(screen.getByRole("button", { name: "Remove profile" }))
  expect(screen.getByText("Will be added when you save")).toBeTruthy()
  expect(screen.getByText("Will be removed when you save")).toBeTruthy()
  expect(add).not.toHaveBeenCalled()
  expect(remove).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Undo removal of Neo" }))
  expect(screen.getByRole("switch", { name: "Enable Neo" })).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Remove profile" }))
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  expect(screen.queryByText("Will be added when you save")).toBeNull()
  expect(screen.getByRole("switch", { name: "Enable Neo" })).toBeTruthy()
  expect(add).not.toHaveBeenCalled()
  expect(remove).not.toHaveBeenCalled()
})

test("Save commits a pending removal and addition and settles the save bar", async () => {
  const remove = vi.spyOn(api.api.letterboxd, "remove").mockResolvedValue({ removed: true })
  const add = vi.spyOn(api.api.letterboxd, "add").mockResolvedValue({ profile: { ...profile, id: "second", displayName: "Trinity", enabled: true } })
  renderProfiles()
  fireEvent.click(screen.getByRole("switch", { name: "Enable Neo" }))
  fireEvent.click(screen.getByRole("button", { name: "Remove profile" }))
  fireEvent.change(screen.getByRole("textbox", { name: "Letterboxd username or profile URL" }), { target: { value: "trinity" } })
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(remove).toHaveBeenCalledWith(profile.id))
  await waitFor(() => expect(add).toHaveBeenCalledWith("trinity"))
  expect(await screen.findByRole("switch", { name: "Enable Trinity" })).toBeTruthy()
  expect(screen.queryByRole("switch", { name: "Enable Neo" })).toBeNull()
  await waitFor(() => expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true))
})

test("a partial save keeps failed additions and does not repeat completed writes", async () => {
  const enable = vi.spyOn(api.api.letterboxd, "setEnabled").mockResolvedValue({ profile: { ...profile, enabled: true } })
  const add = vi.spyOn(api.api.letterboxd, "add")
    .mockResolvedValueOnce({ profile: { ...profile, id: "second", displayName: "Trinity", enabled: true } })
    .mockRejectedValueOnce(new Error("Profile unavailable"))
    .mockResolvedValueOnce({ profile: { ...profile, id: "third", displayName: "Morpheus", enabled: true } })
  renderProfiles()
  fireEvent.click(screen.getByRole("switch", { name: "Enable Neo" }))
  for (const name of ["trinity", "morpheus"]) {
    fireEvent.change(screen.getByRole("textbox", { name: "Letterboxd username or profile URL" }), { target: { value: name } })
    fireEvent.click(screen.getByRole("button", { name: "Add profile" }))
  }
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  expect(await screen.findByRole("alert")).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Cancel adding trinity" })).toBeNull()
  expect(screen.getByRole("button", { name: "Cancel adding morpheus" })).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  expect(await screen.findByRole("switch", { name: "Enable Morpheus" })).toBeTruthy()
  expect(enable).toHaveBeenCalledTimes(1)
  expect(add.mock.calls.map(([input]) => input)).toEqual(["trinity", "morpheus", "morpheus"])
  await waitFor(() => expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true))
})

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
