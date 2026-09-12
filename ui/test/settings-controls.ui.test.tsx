import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { api } from "@/lib/api"
import { queryClient, queryKeys } from "@/lib/query-client"
import { DEFAULT_VIEWING } from "@/lib/viewing"
import Settings from "@/routes/Settings"
import { clientSettingsFixture } from "./support/settings"
import { TestProviders } from "./test-utils"

afterEach(() => { vi.restoreAllMocks(); queryClient.clear() })

function page(route: string) {
  const settings = clientSettingsFixture()
  queryClient.setQueryData(queryKeys.status, { authenticated: true, serverUrl: "https://jellyfin.example", userId: "user", libraryReady: true })
  queryClient.setQueryData(queryKeys.settings, settings)
  queryClient.setQueryData(["viewing", "https://jellyfin.example:user"], { ...DEFAULT_VIEWING })
  queryClient.setQueryData(queryKeys.home, { rows: [], continueWatching: [] })
  queryClient.setQueryData(queryKeys.ratingsStatus, { sources: [], selectionEnabled: false })
  render(<TestProviders client={queryClient} initialEntries={[route]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)
  return settings
}

test.each([
  ["Subtitle size (%)", 50, 200], ["Subtitle outline", 0, 8],
  ["Subtitle background (%)", 0, 100], ["Subtitle vertical position", 0, 100],
  ["Seek backward seconds", 1, 120], ["Seek forward seconds", 1, 120],
])("%s keeps an empty draft, blocks invalid saves, and supports Discard", (label, min, max) => {
  page("/settings/client/playback")
  const save = vi.spyOn(api.settingsPatch, "playback")
  const input = screen.getByRole("spinbutton", { name: label }) as HTMLInputElement
  const original = input.value
  for (const value of ["", String(min - 1), String(max + 1), "1.5"]) {
    fireEvent.change(input, { target: { value } })
    expect(input.value).toBe(value)
    expect(screen.getByRole("alert").textContent).toContain(`from ${min} to ${max}`)
    expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(true)
  }
  expect(save).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  expect(input.value).toBe(original)
  expect(screen.queryByRole("alert")).toBeNull()
})

test("subtitle slider keyboard edits are reflected in exact entry and saved", async () => {
  const settings = page("/settings/client/playback")
  const save = vi.spyOn(api.settingsPatch, "playback").mockImplementation(async (playback) => ({ ...settings, client: { ...settings.client, playback } }))
  fireEvent.keyDown(screen.getByRole("slider", { name: "Subtitle size (%) slider" }), { key: "ArrowRight" })
  expect((screen.getByRole("spinbutton", { name: "Subtitle size (%)" }) as HTMLInputElement).value).toBe("101")
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ comfort: expect.objectContaining({ subtitleSize: 101 }) })))
})

test.each([
  ["Countdown seconds", "61"], ["Episode limit", "21"], ["Text size percent", "79"],
])("Viewing blocks invalid %s and Reset restores valid values", (label, value) => {
  page("/settings/viewing")
  fireEvent.change(screen.getByRole("spinbutton", { name: label }), { target: { value } })
  expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(true)
  expect(screen.getByRole("alert")).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Reset" }))
  expect(screen.queryByRole("alert")).toBeNull()
})

test("Appearance keeps precise delay values and blocks invalid delay or intensity saves", () => {
  page("/settings/appearance")
  const delay = screen.getByRole("spinbutton", { name: "Card preview delay" }) as HTMLInputElement
  fireEvent.change(delay, { target: { value: "775" } })
  expect(delay.value).toBe("775")
  expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(false)
  fireEvent.change(delay, { target: { value: "" } })
  expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(true)
  fireEvent.click(screen.getByRole("switch", { name: "Show pop-out previews on cards" }))
  expect(delay.disabled).toBe(true)
  expect(screen.getByRole("alert").textContent).toContain("from 200 to 2000")
  expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(true)
  fireEvent.click(screen.getByRole("button", { name: "Discard" }))
  fireEvent.change(screen.getByRole("spinbutton", { name: "Artwork intensity" }), { target: { value: "101" } })
  expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(true)
})

test("visible labels operate switches and help text is associated with controls", () => {
  page("/settings/viewing")
  const toggle = screen.getByRole("switch", { name: "Spoiler protection" })
  const original = toggle.getAttribute("aria-checked")
  fireEvent.click(screen.getByText("Spoiler protection"))
  expect(toggle.getAttribute("aria-checked")).not.toBe(original)
  expect(document.getElementById(toggle.getAttribute("aria-describedby") ?? "")?.textContent).toContain("Hide unwatched episode")
})

test("preview scenes support arrow navigation and keep one selected scene", async () => {
  page("/settings/client/playback")
  const day = screen.getByRole("radio", { name: "Day" })
  fireEvent.focus(day)
  fireEvent.keyDown(day, { key: "ArrowRight" })
  const dusk = screen.getByRole("radio", { name: "Dusk" })
  await waitFor(() => expect(document.activeElement).toBe(dusk))
  fireEvent.click(dusk)
  expect(dusk.getAttribute("aria-checked")).toBe("true")
  fireEvent.click(dusk)
  expect(dusk.getAttribute("aria-checked")).toBe("true")
})
