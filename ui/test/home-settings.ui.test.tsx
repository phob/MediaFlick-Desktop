import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { ApiError, api, type HomeSettingsResponse } from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import Settings from "../src/routes/Settings"
import { testQueryClient } from "./test-query-client"
import { appStatus } from "./support/fixtures"
import { TestProviders } from "./test-utils"

const settings: HomeSettingsResponse = {
  collectionMode: "mediaFlick",
  settings: {
    billboard: true,
    watching: { continueWatching: true, nextUp: true, combine: true },
    elements: [
      { kind: "builtIn", id: "watching", enabled: true, label: "Watching", available: true, category: "Built-in" },
      { kind: "genre", id: "Action", enabled: true, label: "Action", available: true, category: "Genre" },
      { kind: "genre", id: "Drama", enabled: false, label: "Drama", available: false, category: "Genre" },
    ],
  },
  defaults: {
    billboard: true,
    watching: { continueWatching: true, nextUp: true, combine: true },
    elements: [
      { kind: "builtIn", id: "watching", enabled: true, label: "Watching", available: true, category: "Built-in" },
      { kind: "genre", id: "Action", enabled: true, label: "Action", available: true, category: "Genre" },
      { kind: "genre", id: "Drama", enabled: false, label: "Drama", available: false, category: "Genre" },
    ],
  },
}

afterEach(() => vi.restoreAllMocks())

test("dragging a Home shelf above another stages the new order until Save", async () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true }))
  client.setQueryData(queryKeys.homeSettings, settings)
  const save = vi.spyOn(api, "saveHomeSettings").mockResolvedValue(settings)

  render(
    <TestProviders client={client} initialEntries={["/settings/home"]}>
        <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  const watchingRow = screen.getByRole("button", { name: "Drag Watching" }).closest("[data-home-element-key]")
  if (!watchingRow) throw new Error("Watching row not found")
  watchingRow.getBoundingClientRect = () => ({ left: 20, top: 100, width: 500, height: 50, right: 520, bottom: 150, x: 20, y: 100, toJSON: () => ({}) })
  fireEvent.pointerDown(screen.getByRole("button", { name: "Drag Action" }), { button: 0, pointerId: 1, clientX: 30, clientY: 170 })
  fireEvent.pointerMove(window, { pointerId: 1, clientX: 30, clientY: 90 })
  fireEvent.pointerUp(window, { pointerId: 1 })
  expect(save).not.toHaveBeenCalled()

  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({
    elements: [
      { kind: "genre", id: "Action", enabled: true },
      { kind: "builtIn", id: "watching", enabled: true },
      { kind: "genre", id: "Drama", enabled: false },
    ],
  })))
})

test("background sync adding and removing Home options reconciles a dirty draft so Save succeeds", async () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true }))
  client.setQueryData(queryKeys.homeSettings, settings)
  const synced: HomeSettingsResponse = {
    ...settings,
    settings: {
      ...settings.settings,
      elements: [
        { kind: "builtIn", id: "watching", enabled: true, label: "Watching", available: true, category: "Built-in" },
        { kind: "genre", id: "Action", enabled: true, label: "Action", available: true, category: "Genre" },
        { kind: "genre", id: "Comedy", enabled: false, label: "Comedy", available: true, category: "Genre" },
      ],
    },
  }
  const save = vi.spyOn(api, "saveHomeSettings").mockImplementation(async (write) => ({
    ...synced,
    settings: {
      ...synced.settings,
      elements: write.elements.map((element) => {
        const current = synced.settings.elements.find((candidate) => candidate.kind === element.kind && candidate.id === element.id)
        if (!current) throw new ApiError("Home options changed while the page was open", 409)
        return { ...current, enabled: element.enabled }
      }),
    },
  }))

  render(
    <TestProviders client={client} initialEntries={["/settings/home"]}>
      <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  fireEvent.click(screen.getByRole("button", { name: "Move Action up" }))
  act(() => {
    client.setQueryData(queryKeys.homeSettings, synced)
  })

  expect(await screen.findByText("Comedy")).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({
    elements: [
      { kind: "genre", id: "Action", enabled: true },
      { kind: "builtIn", id: "watching", enabled: true },
      { kind: "genre", id: "Comedy", enabled: false },
    ],
  })))
})

test("a 409 conflict on Home Save refreshes and reconciles options so the next Save succeeds without Discard", async () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, appStatus({ authenticated: true }))
  client.setQueryData(queryKeys.homeSettings, settings)
  const synced: HomeSettingsResponse = {
    ...settings,
    settings: {
      ...settings.settings,
      elements: [
        { kind: "builtIn", id: "watching", enabled: true, label: "Watching", available: true, category: "Built-in" },
        { kind: "genre", id: "Action", enabled: true, label: "Action", available: true, category: "Genre" },
        { kind: "genre", id: "Comedy", enabled: false, label: "Comedy", available: true, category: "Genre" },
      ],
    },
  }
  vi.spyOn(api, "homeSettings").mockResolvedValue(synced)
  let serverIds = new Set(synced.settings.elements.map((element) => `${element.kind}:${element.id}`))
  const save = vi.spyOn(api, "saveHomeSettings").mockImplementation(async (write) => {
    const requestIds = new Set(write.elements.map((element) => `${element.kind}:${element.id}`))
    if (requestIds.size !== serverIds.size || [...requestIds].some((id) => !serverIds.has(id))) {
      throw new ApiError("Home options changed while the page was open", 409)
    }
    return {
      ...synced,
      settings: {
        ...synced.settings,
        elements: write.elements.map((element) => {
          const current = synced.settings.elements.find((candidate) => candidate.kind === element.kind && candidate.id === element.id)!
          return { ...current, enabled: element.enabled }
        }),
      },
    }
  })

  render(
    <TestProviders client={client} initialEntries={["/settings/home"]}>
      <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  fireEvent.click(screen.getByRole("checkbox", { name: "Show Action" }))
  fireEvent.click(screen.getByRole("button", { name: "Save" }))
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2))
  expect(await screen.findByText("Comedy")).toBeTruthy()
  expect(save).toHaveBeenLastCalledWith(expect.objectContaining({
    elements: [
      { kind: "builtIn", id: "watching", enabled: true },
      { kind: "genre", id: "Action", enabled: false },
      { kind: "genre", id: "Comedy", enabled: false },
    ],
  }))
})
