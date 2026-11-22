import { DEFAULT_COMFORT } from "@/lib/viewing"
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, Status } from "@/lib/api"
import { api } from "@/lib/api"
import { useStatus } from "@/lib/session-queries"
import { createQueryClient, queryKeys } from "@/lib/query-client"
import Settings from "@/routes/Settings"
import { appStatus } from "./support/fixtures"
import { TestProviders } from "./test-utils"

const queryClient = createQueryClient()

const settings: ClientSettings = {
  client: {
    player: {
      playerBackend: "mpv",
      mpvPath: null,
      defaultFullscreen: "fullscreen",
      markWatchedNext: "w",
      playerConfigured: false, comfort: DEFAULT_COMFORT,
    },
    playback: {
      streamingQuality: "original",
      skipIntro: "prompt",
      skipCredits: "prompt",
      skipRecap: "prompt",
      skipCommercial: "prompt",
    },
    application: { closeBehavior: "exit_app", showScrollbars: false, logLevel: "debug" },
  },
  appearance: {
    accent: "signal",
    density: "comfortable",
    artworkIntensity: 100,
    backdropIntensity: 100,
    reducedMotion: false,
    cardPreviews: true,
    showMediaInfo: true,
    ratingSources: [],
  },
  capabilities: { platform: "windows", libmpv: true, integratedLibmpvOverlay: false, mpvInstaller: true },
  recoveries: [],
  serverUrl: "https://jellyfin.example",
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

  test("prompts before deleting local account data when the Application shelf has unsaved changes", async () => {
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
    const trigger = screen.getByRole("button", { name: "Delete local account data" })
    fireEvent.click(trigger)
    const confirmDialog = await screen.findByRole("alertdialog", { name: "Delete local account data?" })
    fireEvent.click(within(confirmDialog).getByRole("button", { name: "Delete local account data" }))

    const leaveDialog = await screen.findByRole("dialog", { name: "Leave without saving?" })
    expect(remove).not.toHaveBeenCalled()
    fireEvent.click(within(leaveDialog).getByRole("button", { name: "Keep editing" }))

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Leave without saving?" })).toBeNull())
    expect(remove).not.toHaveBeenCalled()
    expect(screen.getByRole("switch", { name: "Show scrollbars" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.getByText("You have unsaved changes.")).toBeTruthy()

    fireEvent.click(screen.getByRole("button", { name: "Delete local account data" }))
    const confirmAgain = await screen.findByRole("alertdialog", { name: "Delete local account data?" })
    fireEvent.click(within(confirmAgain).getByRole("button", { name: "Delete local account data" }))

    const leaveAgain = await screen.findByRole("dialog", { name: "Leave without saving?" })
    fireEvent.click(within(leaveAgain).getByRole("button", { name: "Discard and leave" }))

    expect(await screen.findByText("Signed out")).toBeTruthy()
    await waitFor(() => expect(queryClient.getQueryData(queryKeys.item("shared-id"))).toBeUndefined())
    expect(remove).toHaveBeenCalledTimes(1)
  })
})
