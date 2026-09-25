import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { api, type ClientSettings } from "@/lib/api"
import { createQueryClient, queryKeys } from "@/lib/query-client"
import { DEFAULT_VIEWING } from "@/lib/viewing"
import Settings from "@/routes/Settings"
import { clientSettingsFixture } from "./support/settings"
import { TestProviders } from "./test-utils"

const queryClient = createQueryClient()

afterEach(() => { vi.restoreAllMocks(); queryClient.clear() })

function page(route: string, platform: "windows" | "macos" = "windows", cached: (settings: ClientSettings) => ClientSettings = (settings) => settings) {
  const settings = clientSettingsFixture()
  settings.capabilities.platform = platform
  if (platform === "macos") { settings.capabilities.libmpv = false; settings.client.player.playerBackend = "mpv" }
  queryClient.setQueryData(queryKeys.status, { authenticated: true, serverUrl: "https://jellyfin.example", userId: "user", libraryReady: true })
  queryClient.setQueryData(queryKeys.settings, cached(settings))
  queryClient.setQueryData(queryKeys.viewing("https://jellyfin.example:user"), { ...DEFAULT_VIEWING })
  queryClient.setQueryData(queryKeys.home, { rows: [], continueWatching: [] })
  queryClient.setQueryData(queryKeys.ratingsStatus, { sources: [], selectionEnabled: false })
  render(<TestProviders client={queryClient} initialEntries={[route]}>
    <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
  </TestProviders>)
  return settings
}

test.each([
  ["Subtitle size (%)", 50, 200],
])("%s keeps an empty draft, blocks invalid saves, and supports Discard", (label, min, max) => {
  page("/settings/client/player")
  const save = vi.spyOn(api.settingsPatch, "player")
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
  const settings = page("/settings/client/player")
  const save = vi.spyOn(api.settingsPatch, "player").mockImplementation(async (player) => ({ ...settings, client: { ...settings.client, player: {...settings.client.player, ...player} } }))
  fireEvent.keyDown(screen.getByRole("slider", { name: "Subtitle size (%) slider" }), { key: "ArrowRight" })
  expect((screen.getByRole("spinbutton", { name: "Subtitle size (%)" }) as HTMLInputElement).value).toBe("101")
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ comfort: expect.objectContaining({ subtitleSize: 101 }) })))
})

test.each([
  ["Episode limit", "21"],
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

test("shortcut drafts reject conflicts, Discard restores, and Save and Reset use the shelf workflow", async () => {
  const settings = page("/settings/client/player")
  const save = vi.spyOn(api.settingsPatch, "player").mockImplementation(async (player) => ({...settings, client:{...settings.client, player:{...settings.client.player, ...player}}}))
  const recorder = screen.getByRole("button", {name:"Stop playback key"})
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, {key:"w"})
  expect(screen.getByRole("alert").textContent).toContain("conflicts")
  expect(screen.getByRole("button", {name:"Save"}).hasAttribute("disabled")).toBe(true)
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect(recorder.textContent).toBe("Q")
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, {key:"x", ctrlKey:true})
  expect(save).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({comfort:expect.objectContaining({stopKey:"Ctrl+x"})})))
  await waitFor(() => expect(screen.getByRole("button", {name:"Save"}).hasAttribute("disabled")).toBe(true))
  fireEvent.click(screen.getByRole("button", {name:"Reset"}))
  expect(recorder.textContent).toBe("Q")
  expect(save).toHaveBeenCalledTimes(1)
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect(recorder.textContent).toBe("Ctrl + X")
})

test.each(["windows", "macos"] as const)("the watched-next recorder saves and clears combinations on %s", async (platform) => {
  const settings = page("/settings/client/player", platform)
  const save = vi.spyOn(api.settingsPatch, "player").mockImplementation(async (player) => ({...settings, client:{...settings.client, player:{...settings.client.player, ...player}}}))
  const recorder = screen.getByRole("button", {name:"Mark watched key"})
  fireEvent.click(recorder)
  fireEvent.keyDown(recorder, {key:"w", metaKey:platform === "macos", ctrlKey:platform === "windows"})
  expect(save).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({markWatchedNext:platform === "macos" ? "Meta+w" : "Ctrl+w"})))
  await waitFor(() => expect(screen.getByRole("button", {name:"Save"}).hasAttribute("disabled")).toBe(true))
  fireEvent.click(screen.getByRole("button", {name:"Clear mark watched key"}))
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenLastCalledWith(expect.objectContaining({markWatchedNext:null})))
})

test("Player groups all shortcuts and saves a W/stop swap in one request", async () => {
  const settings = page("/settings/client/player")
  const save = vi.spyOn(api.settingsPatch, "player").mockImplementation(async (player) => ({...settings, client:{...settings.client, player:{...settings.client.player, ...player}}}))
  const playbackSave = vi.spyOn(api.settingsPatch, "playback")
  const section = screen.getByText("Keyboard shortcuts").closest("[data-slot=card]")
  expect(section).not.toBeNull()
  const shortcuts = within(section as HTMLElement)
  const watched = shortcuts.getByRole("button", {name:"Mark watched key"})
  const stop = shortcuts.getByRole("button", {name:"Stop playback key"})
  fireEvent.click(watched)
  fireEvent.keyDown(watched, {key:"q"})
  expect(screen.getByRole("button", {name:"Save"}).hasAttribute("disabled")).toBe(true)
  fireEvent.click(stop)
  fireEvent.keyDown(stop, {key:"w"})
  expect(screen.queryByRole("alert")).toBeNull()
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({markWatchedNext:"q", comfort:expect.objectContaining({stopKey:"w"})})))
  expect(save).toHaveBeenCalledTimes(1)
  expect(playbackSave).not.toHaveBeenCalled()
})

/** Mirrors `/api/startup`, whose `serde_json::Value` round trip sorts object keys. */
function sortedKeys<T>(value: T): T {
  if (Array.isArray(value)) return value.map(sortedKeys) as T
  if (typeof value !== "object" || value === null) return value
  return Object.fromEntries(Object.entries(value).sort(([left], [right]) => left.localeCompare(right)).map(([key, entry]) => [key, sortedKeys(entry)])) as T
}

test("Save clears the draft when the response orders keys differently from the cached settings", async () => {
  const settings = page("/settings/client/player", "windows", sortedKeys)
  const save = vi.spyOn(api.settingsPatch, "player").mockImplementation(async (player) => ({...settings, client:{...settings.client, player:{...player, comfort:{...settings.client.player.comfort}, playerConfigured: true}}}))
  fireEvent.click(screen.getByRole("combobox", {name:"Player backend"}))
  fireEvent.click(await screen.findByRole("option", {name:"External mpv"}))
  fireEvent.change(screen.getByRole("textbox", {name:"mpv executable"}), {target:{value:"C:\\mpv\\mpv.exe"}})
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(screen.getByRole("button", {name:"Save"}).hasAttribute("disabled")).toBe(true))
})

test("backend selection changes the visible controls immediately and retains the built-in draft", async () => {
  page("/settings/client/player")
  const stop = screen.getByRole("button", {name:"Stop playback key"})
  fireEvent.click(stop)
  fireEvent.keyDown(stop, {key:"x"})
  const selectBackend = async (name: string) => {
    fireEvent.click(screen.getByRole("combobox", {name:"Player backend"}))
    fireEvent.click(await screen.findByRole("option", {name}))
  }
  await selectBackend("External mpv")
  expect(screen.queryByRole("button", {name:"Stop playback key"})).toBeNull()
  expect(screen.queryByRole("spinbutton", {name:"Subtitle size (%)"})).toBeNull()
  // External mpv keeps only the mark-watched-and-play-next binding.
  expect(screen.getAllByRole("button", {name:/^(?!Clear\b).+ key$/})).toEqual([screen.getByRole("button", {name:"Mark watched key"})])
  await selectBackend("Built-in player")
  expect(screen.getByRole("button", {name:"Stop playback key"}).textContent).toBe("X")
  fireEvent.click(screen.getByRole("button", {name:"Discard"}))
  expect(screen.getByRole("button", {name:"Stop playback key"}).textContent).toBe("Q")
})

test("the mpv file picker fills only its own request's path into the draft", async () => {
  page("/settings/client/player")
  fireEvent.click(screen.getByRole("combobox", {name:"Player backend"}))
  fireEvent.click(await screen.findByRole("option", {name:"External mpv"}))
  const picker = vi.spyOn(api.shell, "filePicker").mockImplementation(async (requestId) => ({ requestId, queued: true }))
  const choose = await screen.findByRole("button", {name:"Choose mpv executable"})
  fireEvent.click(choose)
  await waitFor(() => expect(picker).toHaveBeenCalledTimes(1))
  const requestId = picker.mock.calls[0]?.[0]
  expect(choose.hasAttribute("disabled")).toBe(true)
  const complete = (id: string | undefined, path: string) => window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", {
    detail: { type: "file-picker-completed", payload: { requestId: id, path, error: null } },
  }))

  complete("another-request", "C:/other/mpv.exe")
  expect((screen.getByRole("textbox", {name:"mpv executable"}) as HTMLInputElement).value).toBe("")
  complete(requestId, "C:/mpv/mpv.exe")

  await waitFor(() => expect((screen.getByRole("textbox", {name:"mpv executable"}) as HTMLInputElement).value).toBe("C:/mpv/mpv.exe"))
  expect(choose.hasAttribute("disabled")).toBe(false)
})

test("Playback saves quality without saving Player settings", async () => {
  const settings = page("/settings/client/playback")
  const save = vi.spyOn(api.settingsPatch, "playback").mockImplementation(async (playback) => ({...settings, client:{...settings.client, playback}}))
  const playerSave = vi.spyOn(api.settingsPatch, "player")
  expect(screen.queryByText("Keyboard shortcuts")).toBeNull()
  expect(screen.queryByRole("spinbutton", {name:"Subtitle size (%)"})).toBeNull()
  const quality = screen.getByRole("combobox", {name:"Default streaming quality"})
  fireEvent.click(quality)
  fireEvent.click(await screen.findByRole("option", {name:"Auto"}))
  fireEvent.click(screen.getByRole("button", {name:"Save"}))
  await waitFor(() => expect(save).toHaveBeenCalledWith({...settings.client.playback, streamingQuality:"auto"}))
  expect(playerSave).not.toHaveBeenCalled()
})

// README: original-quality direct playback is the default in both player modes.
test("Playback Reset restores original streaming quality and saves it", async () => {
  const settings = page("/settings/client/playback", "windows", (cached) => ({
    ...cached,
    client: { ...cached.client, playback: { ...cached.client.playback, streamingQuality: "20_mbps" } },
  }))
  const save = vi.spyOn(api.settingsPatch, "playback").mockImplementation(async (playback) => ({ ...settings, client: { ...settings.client, playback } }))
  fireEvent.click(screen.getByRole("button", { name: "Reset" }))
  expect(save).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ streamingQuality: "original" })))
})
