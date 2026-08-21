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
