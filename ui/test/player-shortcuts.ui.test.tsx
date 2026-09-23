import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { QueryClientProvider } from "@tanstack/react-query"
import { afterEach, expect, test, vi } from "vitest"
import { PlayerBar } from "@/components/PlayerBar"
import { api, playerSettingsWrite, type PlayerComfort, type PlayerSettings, type PlayerState } from "@/lib/api"
import { normalizeShortcut, shortcutError } from "@/lib/player-shortcuts"
import { DEFAULT_COMFORT } from "@/lib/viewing"
import { queryKeys } from "@/lib/query-client"
import { testQueryClient } from "./test-query-client"
import { clientSettingsFixture } from "./support/settings"
import { playerSnapshot } from "./support/fixtures"
import cases from "./fixtures/player-shortcuts.json"
import playerSettings from "./fixtures/player-settings.json"

afterEach(() => vi.restoreAllMocks())

test("shortcut normalization shares the native contract, including Command combinations", () => {
  for (const entry of cases) expect(normalizeShortcut(entry.input)).toBe(entry.normalized)
  expect(shortcutError({...DEFAULT_COMFORT, pauseKey:"Ctrl+Shift+p"}, "control+P")).toContain("conflicts")
})

test("the Player write contract sends all bindings together and omits computed fields", () => {
  const settings = {...playerSettings, playerConfigured:true} as PlayerSettings
  expect(playerSettingsWrite(settings)).toEqual(playerSettings)
  expect(shortcutError(settings.comfort, settings.markWatchedNext)).toBeNull()
})

test.each([
  ["pauseKey", "pause"], ["muteKey", "set-mute"], ["fullscreenKey", "toggle-fullscreen"],
  ["stopKey", "stop"], ["subtitlesKey", "toggle-subtitles"],
  ["seekBackKey", "seek"], ["seekForwardKey", "seek"],
  ["seekBackThirtyKey", "seek"], ["seekForwardThirtyKey", "seek"],
] as const)("%s follows its saved combination and ignores typing, recording, and repeats", async (field, command) => {
  const settings = clientSettingsFixture()
  const comfort: PlayerComfort = {...DEFAULT_COMFORT, [field]:"Ctrl+Shift+p"}
  settings.client.player.comfort = comfort
  const player: PlayerState = playerSnapshot({active:true, positionMs:60000, durationMs:120000, paused:false, mute:false})
  const client = testQueryClient()
  client.setQueryData(queryKeys.settings, settings)
  client.setQueryData(queryKeys.playerState, player)
  vi.spyOn(api, "settings").mockResolvedValue(settings)
  vi.spyOn(api, "playerState").mockResolvedValue(player)
  const send = vi.spyOn(api, "playerCommand").mockResolvedValue({accepted:true})
  render(<QueryClientProvider client={client}><input aria-label="Typing" /><div data-shortcut-recorder><button>Recording</button></div><PlayerBar /></QueryClientProvider>)
  const combination = {key:"P", ctrlKey:true, shiftKey:true}
  const oldKey = DEFAULT_COMFORT[field]
  fireEvent.keyDown(window, {key:oldKey === "UP" ? "ArrowUp" : oldKey === "DOWN" ? "ArrowDown" : oldKey})
  fireEvent.keyDown(window, {key:"p"})
  fireEvent.keyDown(window, {...combination, repeat:true})
  fireEvent.keyDown(screen.getByRole("textbox", {name:"Typing"}), combination)
  fireEvent.keyDown(screen.getByRole("button", {name:"Recording"}), combination)
  expect(send).not.toHaveBeenCalled()
  fireEvent.keyDown(window, combination)
  await waitFor(() => expect(send).toHaveBeenCalledTimes(1))
  const positions: Partial<Record<keyof PlayerComfort, number>> = {seekBackKey:50000, seekForwardKey:90000, seekBackThirtyKey:30000, seekForwardThirtyKey:90000}
  expect(send).toHaveBeenCalledWith(expect.objectContaining({command, ...(positions[field] === undefined ? {} : {positionMs:positions[field]})}))
})

test("the configured watched-next command uses Command and ignores the old W binding", async () => {
  const settings = clientSettingsFixture()
  settings.client.player.markWatchedNext = "Meta+w"
  const player: PlayerState = playerSnapshot({active:true, positionMs:10000})
  const client = testQueryClient()
  client.setQueryData(queryKeys.settings, settings)
  client.setQueryData(queryKeys.playerState, player)
  vi.spyOn(api, "settings").mockResolvedValue(settings)
  vi.spyOn(api, "playerState").mockResolvedValue(player)
  const send = vi.spyOn(api, "playerCommand").mockResolvedValue({accepted:true})
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)
  fireEvent.keyDown(window, {key:"w"})
  expect(send).not.toHaveBeenCalled()
  fireEvent.keyDown(window, {key:"w", metaKey:true})
  await waitFor(() => expect(send).toHaveBeenCalledWith({command:"mark-watched-next"}))
})

test("control tooltips name the saved bindings and drop cleared ones", () => {
  const settings = clientSettingsFixture()
  settings.client.player.comfort = {...DEFAULT_COMFORT, seekBackKey:"h", pauseKey:"", muteKey:"Ctrl+Shift+m", fullscreenKey:""}
  const player: PlayerState = playerSnapshot({active:true, positionMs:10000, durationMs:120000})
  const client = testQueryClient()
  client.setQueryData(queryKeys.settings, settings)
  client.setQueryData(queryKeys.playerState, player)
  render(<QueryClientProvider client={client}><PlayerBar /></QueryClientProvider>)
  expect(screen.getByRole("button", {name:"Back 10 seconds"}).getAttribute("title")).toBe("Back 10 seconds (← or H)")
  expect(screen.getByRole("button", {name:"Pause"}).getAttribute("title")).toBe("Pause (Space)")
  expect(screen.getByRole("button", {name:"Mute"}).getAttribute("title")).toBe("Mute (Ctrl + Shift + M)")
  expect(screen.getByRole("button", {name:"Toggle fullscreen"}).getAttribute("title")).toBe("Toggle fullscreen")
})
