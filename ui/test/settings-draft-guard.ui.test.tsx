import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { StrictMode, useState } from "react"
import { createMemoryRouter, Route, RouterProvider, Routes, useLocation } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import SettingsDraftGuard from "@/components/SettingsDraftGuard"
import type * as Wire from "@/lib/api/wire.gen"
import { useSettingsDraftGuard } from "@/lib/settings-drafts"
import {
  listenForOpenSettingsBeforeBridge,
  SETTINGS_PATH,
  useShellNavigation,
} from "@/lib/shell-navigation"

function dispatchShellEvent(detail: Wire.ShellEvent) {
  window.dispatchEvent(new CustomEvent("mediaflick-desktop-shell", { detail }))
}

function Shelf({ name, initialDirty = false }: { name: string; initialDirty?: boolean }) {
  const [dirty, setDirty] = useState(initialDirty)
  useSettingsDraftGuard(dirty, false)
  return (
    <div>
      <h1>{name}</h1>
      <span data-testid="draft-state">{dirty ? "dirty" : "clean"}</span>
      <button type="button" onClick={() => setDirty(true)}>Make dirty</button>
    </div>
  )
}

function Surface({ initialPath, initialDirty }: { initialPath: string; initialDirty: boolean }) {
  useShellNavigation()
  const location = useLocation()
  return (
    <>
      <output data-testid="pathname">{location.pathname}</output>
      <Routes>
        <Route path="/library" element={<div>Library</div>} />
        <Route
          path="/settings/*"
          element={
            <SettingsDraftGuard>
              <Routes>
                <Route
                  path="client/player"
                  element={<Shelf name="Player" initialDirty={initialPath === SETTINGS_PATH && initialDirty} />}
                />
                <Route
                  path="client/application"
                  element={
                    <Shelf
                      name="Application"
                      initialDirty={initialPath === "/settings/client/application" && initialDirty}
                    />
                  }
                />
              </Routes>
            </SettingsDraftGuard>
          }
        />
      </Routes>
    </>
  )
}

function Harness({ initialPath, initialDirty = false }: { initialPath: string; initialDirty?: boolean }) {
  const router = createMemoryRouter(
    [{ path: "*", element: <Surface initialPath={initialPath} initialDirty={initialDirty} /> }],
    { initialEntries: [initialPath] },
  )
  return <StrictMode><RouterProvider router={router} /></StrictMode>
}

describe("settings draft guard and open-settings shell event", () => {
  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
  })

  test("open-settings on another dirty settings shelf prompts and navigates instead of closing the window", async () => {
    const closeSpy = vi.spyOn(window, "close").mockImplementation(() => {})
    render(<Harness initialPath="/settings/client/application" initialDirty />)

    act(() => dispatchShellEvent({ type: "open-settings", payload: {} }))

    await screen.findByRole("dialog", { name: "Leave without saving?" })
    fireEvent.click(screen.getByRole("button", { name: "Keep editing" }))

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Leave without saving?" })).toBeNull()
    })
    expect(screen.getByTestId("pathname").textContent).toBe("/settings/client/application")
    expect(screen.getByTestId("draft-state").textContent).toBe("dirty")
    expect(closeSpy).not.toHaveBeenCalled()

    act(() => dispatchShellEvent({ type: "open-settings", payload: {} }))

    await screen.findByRole("dialog", { name: "Leave without saving?" })
    fireEvent.click(screen.getByRole("button", { name: "Discard and leave" }))

    await waitFor(() => {
      expect(screen.getByTestId("pathname").textContent).toBe(SETTINGS_PATH)
    })
    expect(closeSpy).not.toHaveBeenCalled()
  })

  test("open-settings while already on the default settings shelf keeps the dirty draft without prompting or closing", async () => {
    const closeSpy = vi.spyOn(window, "close").mockImplementation(() => {})
    render(<Harness initialPath={SETTINGS_PATH} initialDirty />)

    act(() => dispatchShellEvent({ type: "open-settings", payload: {} }))

    expect(screen.queryByRole("dialog", { name: "Leave without saving?" })).toBeNull()
    expect(screen.getByTestId("pathname").textContent).toBe(SETTINGS_PATH)
    expect(screen.getByTestId("draft-state").textContent).toBe("dirty")
    expect(closeSpy).not.toHaveBeenCalled()
  })

  test("open-settings from outside settings navigates directly to the default settings shelf", async () => {
    render(<Harness initialPath="/library" />)

    act(() => dispatchShellEvent({ type: "open-settings", payload: {} }))

    await waitFor(() => {
      expect(screen.getByTestId("pathname").textContent).toBe(SETTINGS_PATH)
    })
  })

  test("open-settings dispatched before mount navigates once the shell navigation bridge mounts", async () => {
    const stop = listenForOpenSettingsBeforeBridge()
    try {
      dispatchShellEvent({ type: "open-settings", payload: {} })
      render(<Harness initialPath="/library" />)

      await waitFor(() => {
        expect(screen.getByTestId("pathname").textContent).toBe(SETTINGS_PATH)
      })
    } finally {
      stop()
    }
  })

  test("window-close-blocked still closes the window on Discard and leave", async () => {
    const closeSpy = vi.spyOn(window, "close").mockImplementation(() => {})
    render(<Harness initialPath="/settings/client/application" initialDirty />)

    act(() => dispatchShellEvent({ type: "window-close-blocked", payload: {} }))

    await screen.findByRole("dialog", { name: "Leave without saving?" })
    fireEvent.click(screen.getByRole("button", { name: "Keep editing" }))

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Leave without saving?" })).toBeNull()
    })
    expect(closeSpy).not.toHaveBeenCalled()

    act(() => dispatchShellEvent({ type: "window-close-blocked", payload: {} }))

    await screen.findByRole("dialog", { name: "Leave without saving?" })
    fireEvent.click(screen.getByRole("button", { name: "Discard and leave" }))

    expect(closeSpy).toHaveBeenCalledTimes(1)
    const unload = new Event("beforeunload", { cancelable: true })
    window.dispatchEvent(unload)
    expect(unload.defaultPrevented).toBe(false)
  })
})
