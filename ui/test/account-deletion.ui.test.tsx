import { QueryClientProvider } from "@tanstack/react-query"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { MemoryRouter, Route, Routes } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import type { ClientSettings, Status } from "@/lib/api"
import { api } from "@/lib/api"
import { useStatus } from "@/lib/queries"
import { queryClient, queryKeys } from "@/lib/query-client"
import Settings from "@/routes/Settings"

const settings: ClientSettings = {
  client: {
    player: {
      playerBackend: "mpv",
      mpvPath: null,
      mpchcPath: null,
      defaultFullscreen: "fullscreen",
      markWatchedNext: "w",
      playerConfigured: false,
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
    theme: "system",
    accent: "signal",
    density: "comfortable",
    artworkIntensity: 100,
    backdropIntensity: 100,
    reducedMotion: false,
    cardPreviews: true,
    showMediaInfo: true,
    ratingSources: [],
  },
  capabilities: { platform: "windows", mpchc: true, mpvInstaller: true },
  streamingQuality: "original",
  playerBackend: "mpv",
  playerConfigured: false,
  serverUrl: "https://jellyfin.example",
}

const authenticated: Status = {
  authenticated: true,
  serverUrl: "https://jellyfin.example",
  userId: "user-a",
  userName: "Alice",
}

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
  test("replaces the active status before removing account query data", async () => {
    queryClient.setQueryData(queryKeys.status, authenticated)
    queryClient.setQueryData(queryKeys.settings, settings)
    queryClient.setQueryData(queryKeys.item("shared-id"), { id: "shared-id", name: "Alice's item" })
    vi.spyOn(api, "settings").mockResolvedValue(settings)
    vi.spyOn(api.collections, "deleteLocalAccount").mockResolvedValue(anonymous)
    vi.spyOn(window, "confirm").mockReturnValue(true)

    render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={["/settings/client/application"]}>
          <Routes>
            <Route path="/settings/*" element={<AuthenticatedShell />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    )

    fireEvent.change(await screen.findByLabelText("Type DELETE to confirm local account deletion"), {
      target: { value: "DELETE" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Delete local account data" }))

    expect(await screen.findByText("Signed out")).toBeTruthy()
    await waitFor(() => expect(queryClient.getQueryData(queryKeys.item("shared-id"))).toBeUndefined())
    expect(queryClient.getQueryData<Status>(queryKeys.status)?.authenticated).toBe(false)
  })
})
