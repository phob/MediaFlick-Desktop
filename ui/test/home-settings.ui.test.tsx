import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, expect, test, vi } from "vitest"
import { api, type HomeSettingsResponse } from "../src/lib/api"
import { queryKeys } from "../src/lib/query-client"
import Settings from "../src/routes/Settings"
import { testQueryClient } from "./test-query-client"
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
  client.setQueryData(queryKeys.status, { authenticated: true })
  client.setQueryData(queryKeys.homeSettings, settings)
  const save = vi.spyOn(api, "saveHomeSettings").mockResolvedValue(settings)

  render(
    <TestProviders client={client} initialEntries={["/settings/home"]}>
        <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  expect(screen.queryByText("Drama")).toBeNull()
  // jsdom has no layout, so give the drop target a position above the pointer.
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
