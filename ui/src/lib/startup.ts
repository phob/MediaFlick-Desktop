export interface StartupReadiness {
  statusPending: boolean
  settingsPending: boolean
  /** The first catalog page is still on its way; a failed one has a screen of its own. */
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

/** The native main window starts hidden; the first ready report shows it. */
let revealed = false

export function windowRevealed() {
  return revealed
}

export function markWindowRevealed() {
  revealed = true
}
