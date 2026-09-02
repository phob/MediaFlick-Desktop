import { fireEvent, render, screen } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { expect, test } from "vitest"
import type { HomeSettingsResponse } from "../src/lib/api"
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

test("Home settings reserve a drop position while a shelf follows the pointer", () => {
  const client = testQueryClient()
  client.setQueryData(queryKeys.status, { authenticated: true })
  client.setQueryData(queryKeys.homeSettings, settings)

  render(
    <TestProviders client={client} initialEntries={["/settings/home"]}>
        <Routes><Route path="/settings/*" element={<Settings />} /></Routes>
    </TestProviders>,
  )

  expect(screen.queryByText("Drama")).toBeNull()
  const moveActionUp = screen.getByRole("button", { name: "Move Action up" })
  expect((moveActionUp as HTMLButtonElement).disabled).toBe(false)
  const actionHandle = screen.getByRole("button", { name: "Drag Action" })
  const actionRow = actionHandle.closest(".rounded-lg")
  const watchingRow = screen.getByText("Watching").closest(".rounded-lg")
  if (!actionRow || !watchingRow) throw new Error("Home rows not found")
  actionRow.getBoundingClientRect = () => ({ left: 20, top: 160, width: 500, height: 50, right: 520, bottom: 210, x: 20, y: 160, toJSON: () => ({}) })
  watchingRow.getBoundingClientRect = () => ({ left: 20, top: 100, width: 500, height: 50, right: 520, bottom: 150, x: 20, y: 100, toJSON: () => ({}) })
  fireEvent.pointerDown(actionHandle, { button: 0, pointerId: 1, clientX: 30, clientY: 170 })
  expect(screen.getByTestId("home-drag-preview")).toBeTruthy()
  expect(screen.getByTestId("home-drop-placeholder").style.height).toBe("50px")
  fireEvent.pointerMove(window, { pointerId: 1, clientX: 30, clientY: 90 })
  expect(screen.getByTestId("home-drop-placeholder").nextElementSibling?.textContent).toContain("Watching")
  fireEvent.pointerUp(window, { pointerId: 1 })
  expect((screen.getByRole("button", { name: "Move Action up" }) as HTMLButtonElement).disabled).toBe(true)
})
