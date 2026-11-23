import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { StrictMode, useState } from "react"
import { createMemoryRouter, Route, RouterProvider, Routes, useLocation } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { AppShell } from "@/components/AppShell"
import SettingsDraftGuard from "@/components/SettingsDraftGuard"
import { api, type ClientSettings, type Status } from "@/lib/api"
import type * as Wire from "@/lib/api/wire.gen"
import { queryKeys } from "@/lib/query-client"
import { useSettingsDraftGuard } from "@/lib/settings-drafts"
import {
  listenForOpenSettingsBeforeBridge,
  SETTINGS_PATH,
  useShellNavigation,
} from "@/lib/shell-navigation"
import { DEFAULT_VIEWING } from "@/lib/viewing"
import Settings from "@/routes/Settings"
import { appStatus, librariesResponse, playerSnapshot } from "./support/fixtures"
import { clientSettingsFixture } from "./support/settings"
import { testQueryClient } from "./test-query-client"
import { TestProviders } from "./test-utils"

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

function stubDesktopViewport() {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      media: "",
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => false,
    })),
  )
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value() {},
  })
}

function seedShellSession(settings: ClientSettings) {
  const signedIn: Status = appStatus({
    authenticated: true,
    serverUrl: "https://jellyfin.example",
    userId: "user-a",
    userName: "Alice",
  })
  const signedOut: Status = {
    ...signedIn,
    authenticated: false,
    userId: null,
    userName: null,
  }
  const client = testQueryClient()
  client.setQueryDefaults(queryKeys.status, { staleTime: Infinity })
  client.setQueryDefaults(queryKeys.settings, { staleTime: Infinity })
  client.setQueryDefaults(queryKeys.root("viewing"), { staleTime: Infinity })
  client.setQueryDefaults(queryKeys.libraries, { staleTime: Infinity })
  client.setQueryDefaults(queryKeys.companion, { staleTime: Infinity })
  client.setQueryDefaults(queryKeys.playerState, { staleTime: Infinity })
  client.setQueryDefaults(queryKeys.root("collections"), { staleTime: Infinity })
  client.setQueryData(queryKeys.status, signedIn)
  client.setQueryData(queryKeys.settings, settings)
  client.setQueryData(queryKeys.viewing("https://jellyfin.example:user-a"), DEFAULT_VIEWING)
  client.setQueryData(queryKeys.libraries, librariesResponse())
  client.setQueryData(queryKeys.playerState, playerSnapshot())
  client.setQueryData(queryKeys.companion, {
    available: false,
    compatible: false,
    checked: true,
    info: null,
    error: null,
    supportedApi: { min: 1, max: 1 },
    serverTooOld: null,
  })
  client.setQueryData(queryKeys.collectionSettings("https://jellyfin.example:user-a"), {
    modeSelection: "jellyfin",
    effectiveMode: "jellyfin",
    mediaFlickAvailable: false,
    franchises: { includeUnreleased: false },
  })
  vi.spyOn(api, "settings").mockResolvedValue(settings)
  return { client, signedIn, signedOut }
}

async function selectSidebarSignOut() {
  const trigger = await screen.findByRole("button", { name: /Alice/ })
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: "mouse" })
  fireEvent.click(await screen.findByRole("menuitem", { name: "Sign out" }))
}

describe("settings draft guard on sidebar sign-out and account change", () => {
  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  test("signing out from the sidebar with a dirty device-level Settings shelf prompts before logging out", async () => {
    stubDesktopViewport()
    const settings = clientSettingsFixture()
    const { client, signedOut } = seedShellSession(settings)
    const logout = vi.spyOn(api, "logout").mockResolvedValue(signedOut)

    render(
      <TestProviders client={client} initialEntries={["/settings/client/application"]}>
        <AppShell>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
        </AppShell>
      </TestProviders>,
    )

    const scrollbars = await screen.findByRole("switch", { name: "Show scrollbars" })
    fireEvent.click(scrollbars)
    await waitFor(() => expect(scrollbars.getAttribute("aria-checked")).toBe("true"))
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    await selectSidebarSignOut()

    const dialog = await screen.findByRole("dialog", { name: "Sign out without saving?" })
    expect(logout).not.toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep editing" }))

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Sign out without saving?" })).toBeNull())
    expect(logout).not.toHaveBeenCalled()
    expect(screen.getByRole("switch", { name: "Show scrollbars" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    await selectSidebarSignOut()

    const dialogAgain = await screen.findByRole("dialog", { name: "Sign out without saving?" })
    fireEvent.click(within(dialogAgain).getByRole("button", { name: "Discard and sign out" }))

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1))
    await screen.findByRole("link", { name: "Back to sign in" })
    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "Show scrollbars" }).getAttribute("aria-checked")).toBe("false")
    })
    expect(screen.queryByText("You have unsaved changes.")).toBeNull()
  })

  test("signing out from the sidebar with a dirty account-scoped Settings shelf prompts and discards on confirm", async () => {
    stubDesktopViewport()
    const settings = clientSettingsFixture()
    const { client, signedOut } = seedShellSession(settings)
    const logout = vi.spyOn(api, "logout").mockResolvedValue(signedOut)

    render(
      <TestProviders client={client} initialEntries={["/settings/viewing"]}>
        <AppShell>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
        </AppShell>
      </TestProviders>,
    )

    const audioInput = await screen.findByRole("textbox", { name: "Audio languages" })
    fireEvent.change(audioInput, { target: { value: "ja, en" } })
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    await selectSidebarSignOut()

    const dialog = await screen.findByRole("dialog", { name: "Sign out without saving?" })
    expect(logout).not.toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep editing" }))

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Sign out without saving?" })).toBeNull())
    expect(logout).not.toHaveBeenCalled()
    expect((screen.getByRole("textbox", { name: "Audio languages" }) as HTMLInputElement).value).toBe("ja, en")

    await selectSidebarSignOut()

    const dialogAgain = await screen.findByRole("dialog", { name: "Sign out without saving?" })
    fireEvent.click(within(dialogAgain).getByRole("button", { name: "Discard and sign out" }))

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1))
    await screen.findByText("Sign in required")
  })

  test("signing out from the sidebar with a clean Settings shelf logs out without prompting", async () => {
    stubDesktopViewport()
    const settings = clientSettingsFixture()
    const { client, signedOut } = seedShellSession(settings)
    const logout = vi.spyOn(api, "logout").mockResolvedValue(signedOut)

    render(
      <TestProviders client={client} initialEntries={["/settings/client/application"]}>
        <AppShell>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
        </AppShell>
      </TestProviders>,
    )

    await screen.findByRole("switch", { name: "Show scrollbars" })
    await selectSidebarSignOut()

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1))
    expect(screen.queryByRole("dialog", { name: "Sign out without saving?" })).toBeNull()
  })

  test("signing out while a Settings shelf is saving waits for the save before proceeding", async () => {
    stubDesktopViewport()
    const settings = clientSettingsFixture()
    const { client, signedOut } = seedShellSession(settings)
    const logout = vi.spyOn(api, "logout").mockResolvedValue(signedOut)
    let resolveSave: ((saved: ClientSettings) => void) | null = null
    vi.spyOn(api.settingsPatch, "application").mockImplementation(
      () => new Promise<ClientSettings>((resolve) => {
        resolveSave = resolve
      }),
    )

    render(
      <TestProviders client={client} initialEntries={["/settings/client/application"]}>
        <AppShell>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
        </AppShell>
      </TestProviders>,
    )

    const scrollbars = await screen.findByRole("switch", { name: "Show scrollbars" })
    fireEvent.click(scrollbars)
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await screen.findByRole("button", { name: "Saving…" })

    await selectSidebarSignOut()

    const dialog = await screen.findByRole("dialog", { name: "Saving settings…" })
    expect(dialog.textContent).toContain("Wait for the save to finish before signing out.")
    expect(logout).not.toHaveBeenCalled()
    expect(within(dialog).getByRole("button", { name: "Discard and sign out" }).hasAttribute("disabled")).toBe(true)

    const savedSettings: ClientSettings = {
      ...settings,
      client: {
        ...settings.client,
        application: { ...settings.client.application, showScrollbars: true },
      },
    }
    act(() => resolveSave?.(savedSettings))

    const readyDialog = await screen.findByRole("dialog", { name: "Sign out?" })
    const proceed = within(readyDialog).getByRole("button", { name: "Sign out" })
    expect(proceed.hasAttribute("disabled")).toBe(false)
    fireEvent.click(proceed)

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1))
  })

  test("account status changes do not remount device-level Settings shelves and drop their unsaved drafts", async () => {
    const settings = clientSettingsFixture()
    const { client, signedOut } = seedShellSession(settings)

    render(
      <TestProviders client={client} initialEntries={["/settings/client/application"]}>
        <SettingsDraftGuard>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
        </SettingsDraftGuard>
      </TestProviders>,
    )

    const scrollbars = await screen.findByRole("switch", { name: "Show scrollbars" })
    fireEvent.click(scrollbars)
    await waitFor(() => expect(scrollbars.getAttribute("aria-checked")).toBe("true"))
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    act(() => {
      client.setQueryData(queryKeys.status, signedOut)
    })

    await screen.findByRole("link", { name: "Back to sign in" })
    expect(screen.getByRole("switch", { name: "Show scrollbars" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()
  })

  test("Sign out stays disabled while logout is pending so duplicate logouts cannot fire", async () => {
    stubDesktopViewport()
    const settings = clientSettingsFixture()
    const { client, signedOut } = seedShellSession(settings)
    let resolveLogout: ((status: Status) => void) | null = null
    const logout = vi.spyOn(api, "logout").mockImplementation(
      () => new Promise<Status>((resolve) => {
        resolveLogout = resolve
      }),
    )

    render(
      <TestProviders client={client} initialEntries={["/settings/client/application"]}>
        <AppShell>
          <Routes>
            <Route path="/settings/*" element={<Settings />} />
          </Routes>
        </AppShell>
      </TestProviders>,
    )

    await screen.findByRole("switch", { name: "Show scrollbars" })
    await selectSidebarSignOut()
    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1))

    const trigger = await screen.findByRole("button", { name: /Alice/ })
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: "mouse" })
    const signOutItem = await screen.findByRole("menuitem", { name: "Sign out" })
    expect(signOutItem.getAttribute("aria-disabled")).toBe("true")
    fireEvent.click(signOutItem)
    expect(logout).toHaveBeenCalledTimes(1)
    fireEvent.keyDown(signOutItem, { key: "Escape" })

    act(() => resolveLogout?.(signedOut))
    await screen.findByRole("link", { name: "Back to sign in" })
  })
})
