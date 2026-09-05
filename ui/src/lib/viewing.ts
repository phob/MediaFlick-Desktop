import { createContext, useContext } from "react"
import { useQuery } from "@tanstack/react-query"
import { api, type ItemSummary, type PlayerComfort, type ViewingSettings } from "./api"
import { collectionAccountKey, useStatus } from "./queries"

export const DEFAULT_VIEWING: ViewingSettings = {
  spoilerProtection: false, nextEpisode: "auto", countdownSeconds: 10, episodeLimit: 0,
  audioLanguages: [], subtitleLanguages: [], preferOriginalAudio: false, subtitleMode: "server",
  resumeRewindSeconds: 0, textScale: 100, posterSize: 168, previewDelayMs: 550,
  startupDestination: "home", rememberFilters: false, hideWatched: false,
}

export const DEFAULT_COMFORT: PlayerComfort = {
  subtitleSize: 100, subtitleOutline: 3, subtitleBackground: 0, subtitlePosition: 100,
  seekBackSeconds: 10, seekForwardSeconds: 30, pauseKey: "k", muteKey: "m", fullscreenKey: "f",
}

export function useViewing() {
  const { data: status } = useStatus()
  const account = collectionAccountKey(status)
  return useQuery({ queryKey: ["viewing", account], queryFn: api.viewing, enabled: Boolean(status?.authenticated) })
}

export function concealEpisode<T extends ItemSummary>(item: T): T {
  return { ...item, name: "Unwatched episode", originalTitle: null, overview: null, primaryImageTag: null,
    thumbImageTag: null, backdropImageTag: null, logoImageTag: null }
}

export const ViewingContext = createContext<ViewingSettings | null>(DEFAULT_VIEWING)

export function useSpoilerProtection(item: ItemSummary | undefined) {
  const viewing = useContext(ViewingContext)
  // Hide while preferences load so an account's first frame cannot reveal spoilers.
  return Boolean(item?.kind === "Episode" && !item.played && viewing?.spoilerProtection !== false)
}
