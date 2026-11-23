import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, Status } from "@/lib/api"
import { api } from "@/lib/api"
import { useStatus } from "@/lib/session-queries"
import { createQueryClient, queryKeys } from "@/lib/query-client"
import Settings from "@/routes/Settings"
import { appStatus } from "./support/fixtures"
import { clientSettingsFixture } from "./support/settings"
import { TestProviders } from "./test-utils"

const queryClient = createQueryClient()

const baseSettings = clientSettingsFixture()
const settings: ClientSettings = {
  ...baseSettings,
  client: {
    ...baseSettings.client,
    player: {
      ...baseSettings.client.player,
      playerBackend: "mpv",
      playerConfigured: false,
    },
  },
}

const authenticated: Status = appStatus({
  authenticated: true,
  serverUrl: "https://jellyfin.example",
  userId: "user-a",
  userName: "Alice",
})

const anonymous: Status = {
  ...authenticated,
  authenticated: false,
  userId: null,
  userName: null,
}

function AuthenticatedShell() {
  const { data } = useStatus()
  return data?.authenticated ? <Settings /> : <p>Signed out</p>
}

afterEach(() => {
  vi.restoreAllMocks()
  queryClient.clear()
})

describe("local account deletion", () => {
  test("requires confirmation, then signs out and removes the account's cached data", async () => {
    queryClient.setQueryData(queryKeys.status, authenticated)
    queryClient.setQueryData(queryKeys.settings, settings)
    queryClient.setQueryData(queryKeys.item("shared-id"), { id: "shared-id", name: "Alice's item" })
    vi.spyOn(api, "settings").mockResolvedValue(settings)
    const remove = vi.spyOn(api.collections, "deleteLocalAccount").mockResolvedValue(anonymous)

    render(
      <TestProviders client={queryClient} initialEntries={["/settings/client/application"]}>
          <Routes>
            <Route path="/settings/*" element={<AuthenticatedShell />} />
          </Routes>
      </TestProviders>,
    )

    const confirmation = await screen.findByLabelText("Type DELETE to confirm local account deletion")
    const trigger = screen.getByRole("button", { name: "Delete local account data" })
    expect(trigger.hasAttribute("disabled")).toBe(true)
    fireEvent.change(confirmation, {
      target: { value: "DELETE" },
    })
    fireEvent.click(trigger)
    const dialog = await screen.findByRole("alertdialog", { name: "Delete local account data?" })
    expect(remove).not.toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }))
    expect(remove).not.toHaveBeenCalled()

    fireEvent.click(trigger)
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete local account data" }))

    expect(await screen.findByText("Signed out")).toBeTruthy()
    await waitFor(() => expect(queryClient.getQueryData(queryKeys.item("shared-id"))).toBeUndefined())
    expect(remove).toHaveBeenCalledTimes(1)
  })

  test("warns inside the delete confirmation when the Application shelf has unsaved changes and discards on confirm", async () => {
    queryClient.setQueryData(queryKeys.status, authenticated)
    queryClient.setQueryData(queryKeys.settings, settings)
    queryClient.setQueryData(queryKeys.item("shared-id"), { id: "shared-id", name: "Alice's item" })
    vi.spyOn(api, "settings").mockResolvedValue(settings)
    const remove = vi.spyOn(api.collections, "deleteLocalAccount").mockResolvedValue(anonymous)

    render(
      <TestProviders client={queryClient} initialEntries={["/settings/client/application"]}>
        <Routes>
          <Route path="/settings/*" element={<AuthenticatedShell />} />
        </Routes>
      </TestProviders>,
    )

    const scrollbars = await screen.findByRole("switch", { name: "Show scrollbars" })
    fireEvent.click(scrollbars)
    await waitFor(() => expect(scrollbars.getAttribute("aria-checked")).toBe("true"))
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    const confirmation = screen.getByLabelText("Type DELETE to confirm local account deletion")
    fireEvent.change(confirmation, { target: { value: "DELETE" } })
    fireEvent.click(screen.getByRole("button", { name: "Delete local account data" }))

    const confirmDialog = await screen.findByRole("alertdialog", { name: "Delete local account data?" })
    expect(confirmDialog.textContent).toContain("Your unsaved Application settings will also be discarded.")
    expect(remove).not.toHaveBeenCalled()
    fireEvent.click(within(confirmDialog).getByRole("button", { name: "Keep editing" }))

    await waitFor(() => expect(screen.queryByRole("alertdialog", { name: "Delete local account data?" })).toBeNull())
    expect(remove).not.toHaveBeenCalled()
    expect(screen.getByRole("switch", { name: "Show scrollbars" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Delete local account data" }))
    const confirmAgain = await screen.findByRole("alertdialog", { name: "Delete local account data?" })
    fireEvent.click(within(confirmAgain).getByRole("button", { name: "Discard and delete" }))

    expect(await screen.findByText("Signed out")).toBeTruthy()
    await waitFor(() => expect(queryClient.getQueryData(queryKeys.item("shared-id"))).toBeUndefined())
    expect(remove).toHaveBeenCalledTimes(1)
  })

  test("disables confirming local account deletion while Application settings are saving", async () => {
    queryClient.setQueryData(queryKeys.status, authenticated)
    queryClient.setQueryData(queryKeys.settings, settings)
    vi.spyOn(api, "settings").mockResolvedValue(settings)
    const remove = vi.spyOn(api.collections, "deleteLocalAccount").mockResolvedValue(anonymous)
    const deferred: { resolve: ((saved: ClientSettings) => void) | null } = { resolve: null }
    vi.spyOn(api.settingsPatch, "application").mockImplementation(
      () => new Promise<ClientSettings>((resolve) => {
        deferred.resolve = resolve
      }),
    )

    render(
      <TestProviders client={queryClient} initialEntries={["/settings/client/application"]}>
        <Routes>
          <Route path="/settings/*" element={<AuthenticatedShell />} />
        </Routes>
      </TestProviders>,
    )

    fireEvent.click(await screen.findByRole("switch", { name: "Show scrollbars" }))
    fireEvent.change(screen.getByLabelText("Type DELETE to confirm local account deletion"), {
      target: { value: "DELETE" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))
    await screen.findByRole("button", { name: "Saving…" })

    fireEvent.click(screen.getByRole("button", { name: "Delete local account data" }))
    const confirmDialog = await screen.findByRole("alertdialog", { name: "Delete local account data?" })
    expect(confirmDialog.textContent).toContain("Wait for the save to finish before deleting local account data.")
    expect(within(confirmDialog).getByRole("button", { name: "Discard and delete" }).hasAttribute("disabled")).toBe(true)

    act(() => deferred.resolve?.({
      ...settings,
      client: {
        ...settings.client,
        application: { ...settings.client.application, showScrollbars: true },
      },
    }))

    const readyButton = await within(confirmDialog).findByRole("button", { name: "Delete local account data" })
    await waitFor(() => expect(readyButton.hasAttribute("disabled")).toBe(false))
    fireEvent.click(readyButton)

    expect(await screen.findByText("Signed out")).toBeTruthy()
    expect(remove).toHaveBeenCalledTimes(1)
  })
})
