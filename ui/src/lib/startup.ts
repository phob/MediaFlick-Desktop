export interface StartupReadiness {
  statusPending: boolean
  waitingForLibrary: boolean
  showingSettings: boolean
  initialHomeEnabled: boolean
  homePending: boolean
  billboardPending: boolean
}

export function startupScreenReady({
  statusPending,
  waitingForLibrary,
  showingSettings,
  initialHomeEnabled,
  homePending,
  billboardPending,
}: StartupReadiness) {
  const waitingForInitialHome = initialHomeEnabled && (homePending || billboardPending)
  return !statusPending && (!waitingForLibrary || showingSettings) && !waitingForInitialHome
}
