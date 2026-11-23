import { api } from "@/lib/api"

export interface StartupReadiness {
  statusPending: boolean
  settingsPending: boolean
  catalogLoading: boolean
  showingSettings: boolean
  initialHomeEnabled: boolean
  homePending: boolean
  billboardPending: boolean
}

export function startupScreenReady({
  statusPending,
  settingsPending,
  catalogLoading,
  showingSettings,
  initialHomeEnabled,
  homePending,
  billboardPending,
}: StartupReadiness) {
  const waitingForInitialHome = initialHomeEnabled && (homePending || billboardPending)
  return !statusPending && !settingsPending && (!catalogLoading || showingSettings) && !waitingForInitialHome
}

export function shouldRevealLoadingScreen({
  statusPending,
  settingsPending,
  catalogLoading,
  showingSettings,
}: Pick<StartupReadiness, "statusPending" | "settingsPending" | "catalogLoading" | "showingSettings">) {
  return !statusPending && !settingsPending && catalogLoading && !showingSettings
}

let revealed = false

export function windowRevealed() {
  return revealed
}

export function resetWindowRevealed() {
  revealed = false
}

export function signalWindowReady() {
  revealed = true
  void api.shell.windowReady().catch(() => {
    if (window.location.protocol === "mediaflick-desktop:") {
      const beacon = document.createElement("img")
      beacon.hidden = true
      beacon.alt = ""
      beacon.addEventListener("error", () => beacon.remove(), { once: true })
      document.body.append(beacon)
      beacon.src = "mediaflick-desktop://window-ready"
      window.setTimeout(() => beacon.remove(), 1_000)
    }
  })
}
