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

let revealed = false

export function windowRevealed() {
  return revealed
}

export function markWindowRevealed() {
  revealed = true
}
