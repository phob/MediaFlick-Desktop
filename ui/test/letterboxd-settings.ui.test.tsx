import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes, useLocation, useNavigate } from "react-router-dom"
import { toast } from "sonner"
import { afterEach, expect, test, vi } from "vitest"
import type { LetterboxdProfile } from "../src/lib/api"
import * as api from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import Settings from "../src/routes/Settings"
import { testQueryClient } from "./test-query-client"
import { appStatus } from "./support/fixtures"
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
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true, userId: "account-1" }))
  client.setQueryData(queryKeys.letterboxdProfiles, { profiles: [profile] })
  vi.spyOn(api.api.letterboxd, "profiles").mockImplementation(async () => ({
    profiles: client.getQueryData<{ profiles: LetterboxdProfile[] }>(queryKeys.letterboxdProfiles)?.profiles ?? [],
  }))
  render(<TestProviders client={client} initialEntries={["/settings/client/player", "/settings/integrations/letterboxd"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    <NavigationProbe />
  </TestProviders>)
  return client
}

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

test("profile additions and removals can be undone or discarded without writes", () => {
  const add = vi.spyOn(api.api.letterboxd, "add")
  const remove = vi.spyOn(api.api.letterboxd, "remove")
  renderProfiles()
  fireEvent.change(screen.getByRole("textbox", { name: "Letterboxd username or profile URL" }), { target: { value: "trinity" } })
  fireEvent.click(screen.getByRole("button", { name: "Add profile" }))
  fireEvent.click(screen.getByRole("button", { name: "Remove profile" }))
  expect(screen.getByRole("button", { name: "Cancel adding trinity" })).toBeTruthy()
  expect(screen.queryByRole("switch", { name: "Enable Neo" })).toBeNull()
  expect(add).not.toHaveBeenCalled()
  expect(remove).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Undo removal of Neo" }))
  expect(screen.getByRole("switch", { name: "Enable Neo" })).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Remove profile" }))
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  expect(screen.queryByRole("button", { name: "Cancel adding trinity" })).toBeNull()
  expect(screen.getByRole("switch", { name: "Enable Neo" })).toBeTruthy()
  expect(add).not.toHaveBeenCalled()
  expect(remove).not.toHaveBeenCalled()
})

test("Save commits a pending removal and addition", async () => {
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
})

test("Reset restores Letterboxd profile enablement and Save commits it", async () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true }))
  client.setQueryData(queryKeys.letterboxdProfiles, { profiles: [profile] })
  vi.spyOn(api.api.letterboxd, "profiles").mockResolvedValue({ profiles: [{ ...profile, enabled: true }] })
  const setEnabled = vi.spyOn(api.api.letterboxd, "setEnabled").mockResolvedValue({
    profile: { ...profile, enabled: true },
  })

  render(
    <TestProviders client={client} initialEntries={["/settings/integrations/letterboxd"]}>
        <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  fireEvent.click(screen.getByRole("button", { name: "Restore defaults" }))
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(setEnabled).toHaveBeenCalledWith(profile.id, true))
})

test("in-flight refresh returning 404 after removing the profile does not toast an error or resurrect it", async () => {
  let rejectRefresh: (error: Error) => void = () => {}
  const refresh = vi.spyOn(api.api.letterboxd, "refresh").mockImplementation(
    () => new Promise((_, reject) => {
      rejectRefresh = reject
    }),
  )
  const remove = vi.spyOn(api.api.letterboxd, "remove").mockResolvedValue({ removed: true })
  const toastError = vi.spyOn(toast, "error").mockImplementation(() => "")
  const client = renderProfiles()

  fireEvent.click(screen.getByRole("button", { name: "Refresh profile" }))
  await waitFor(() => expect(refresh).toHaveBeenCalledWith(profile.id))

  fireEvent.click(screen.getByRole("button", { name: "Remove profile" }))
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(remove).toHaveBeenCalledWith(profile.id))
  await waitFor(() => expect(screen.queryByRole("switch", { name: "Enable Neo" })).toBeNull())

  rejectRefresh(new api.ApiError("profile not found", 404))
  await waitFor(() =>
    expect(client.getMutationCache().getAll().some((mutation) => mutation.state.status === "error")).toBe(true),
  )
  expect(screen.getByText("No Letterboxd profiles connected yet.")).toBeTruthy()
  expect(screen.queryByRole("switch", { name: "Enable Neo" })).toBeNull()
  expect(toastError).not.toHaveBeenCalled()
})

test("refresh returning 404 for a still-listed profile refreshes the list and reports the error", async () => {
  vi.spyOn(api.api.letterboxd, "refresh").mockRejectedValue(new api.ApiError("profile not found", 404))
  const toastError = vi.spyOn(toast, "error").mockImplementation(() => "")
  const client = renderProfiles()
  const fetchProfiles = vi.spyOn(api.api.letterboxd, "profiles").mockResolvedValue({ profiles: [] })

  fireEvent.click(screen.getByRole("button", { name: "Refresh profile" }))
  await waitFor(() => expect(toastError).toHaveBeenCalledWith("profile not found"))
  await waitFor(() => expect(fetchProfiles).toHaveBeenCalled())
  expect(client.getQueryData<{ profiles: LetterboxdProfile[] }>(queryKeys.letterboxdProfiles)?.profiles).toEqual([])
})

test("in-flight refresh completing after disabling the profile keeps it disabled and updates verification fields", async () => {
  const enabledProfile: LetterboxdProfile = { ...profile, enabled: true, verificationStatus: "unverified" }
  const disabledProfile: LetterboxdProfile = { ...enabledProfile, enabled: false }
  const refreshedProfile: LetterboxdProfile = {
    ...disabledProfile,
    displayName: "Neo Verified",
    verificationStatus: "verified",
    lastCheckedAt: 2,
  }
  let storedProfiles = [enabledProfile]
  let resolveRefresh: (value: { profile: LetterboxdProfile }) => void = () => {}
  const refresh = vi.spyOn(api.api.letterboxd, "refresh").mockImplementation(
    () => new Promise((resolve) => {
      resolveRefresh = resolve
    }),
  )
  const setEnabled = vi.spyOn(api.api.letterboxd, "setEnabled").mockImplementation(async () => {
    storedProfiles = [disabledProfile]
    return { profile: disabledProfile }
  })
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true, userId: "account-1" }))
  client.setQueryData(queryKeys.letterboxdProfiles, { profiles: storedProfiles })
  vi.spyOn(api.api.letterboxd, "profiles").mockImplementation(async () => ({ profiles: storedProfiles }))
  render(<TestProviders client={client} initialEntries={["/settings/integrations/letterboxd"]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)

  fireEvent.click(screen.getByRole("button", { name: "Refresh profile" }))
  await waitFor(() => expect(refresh).toHaveBeenCalledWith(enabledProfile.id))

  fireEvent.click(screen.getByRole("switch", { name: "Enable Neo" }))
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(setEnabled).toHaveBeenCalledWith(enabledProfile.id, false))
  expect(screen.getByRole("switch", { name: "Enable Neo" }).getAttribute("aria-checked")).toBe("false")

  storedProfiles = [refreshedProfile]
  resolveRefresh({ profile: refreshedProfile })
  const updatedSwitch = await screen.findByRole("switch", { name: "Enable Neo Verified" })
  expect(updatedSwitch.getAttribute("aria-checked")).toBe("false")
})
