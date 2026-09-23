export interface StartupReadiness {
  statusPending: boolean
  settingsPending: boolean
  waitingForLibrary: boolean
  showingSettings: boolean
  initialHomeEnabled: boolean
  homePending: boolean
  billboardPending: boolean
}

export function startupScreenReady({
  statusPending,
  settingsPending,
  waitingForLibrary,
  showingSettings,
  initialHomeEnabled,
  homePending,
  billboardPending,
}: StartupReadiness) {
  const waitingForInitialHome = initialHomeEnabled && (homePending || billboardPending)
  return !statusPending && !settingsPending && (!waitingForLibrary || showingSettings) && !waitingForInitialHome
}

/** The native main window starts hidden; the first ready report shows it. */
let revealed = false

export function windowRevealed() {
  return revealed
}

export function markWindowRevealed() {
  revealed = true
}

/** Test hook: treat the next startup as a fresh, still-hidden window. */
export function resetWindowRevealForTests() {
  revealed = false
}
