import { createContext, useContext } from "react"
import { useQuery } from "@tanstack/react-query"
import type { ItemSummary, ViewingSettings } from "./api"
import { accountKey, useStatus } from "./session-queries"
import { viewingQueryOptions } from "./settings-queries"

export const DEFAULT_VIEWING: ViewingSettings = {
  spoilerProtection: false, nextEpisode: "auto", countdownSeconds: 10, episodeLimit: 0,
  audioLanguages: [], subtitleLanguages: [], preferOriginalAudio: false, subtitleMode: "server",
  resumeRewindSeconds: 0, textScale: 100, posterSize: 168, previewDelayMs: 550,
  startupDestination: "home", rememberFilters: false, hideWatched: false,
}

export function useViewing() {
  const { data: status } = useStatus()
  return useQuery({ ...viewingQueryOptions(accountKey(status)), enabled: Boolean(status?.authenticated) })
}

export function concealEpisode<T extends ItemSummary>(item: T): T {
  return { ...item, name: "Unwatched episode", originalTitle: null, overview: null, primaryImageTag: null,
    thumbImageTag: null, backdropImageTag: null, logoImageTag: null }
}

export const ViewingContext = createContext<ViewingSettings | null>(DEFAULT_VIEWING)

export function useSpoilerProtection(item: ItemSummary | undefined) {
  const viewing = useContext(ViewingContext)
  return Boolean(item?.kind === "Episode" && !item.played && viewing?.spoilerProtection !== false)
}
