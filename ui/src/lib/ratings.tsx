import { useQueryClient } from "@tanstack/react-query"
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { api, ApiError, type ItemRatings } from "./api"
import { COALESCE_MS, useCardBatches } from "./card-batches"
import { RatingsContext } from "./rating-context"
import { queryKeys } from "./query-client"
import { useRatingsStatus, useSettings } from "./queries"
import { readShellEvent } from "./shell-events"

const CACHE_RECHECK_SECONDS = 6 * 60 * 60
/**
 * A response can answer without an entry for a requested id — most commonly
 * when another native call still owns that title's refresh, since an aborted
 * request's native work is not cancelled. Ask again a little later, a bounded
 * number of times, instead of treating the omission as final for the mount.
 */
const OMITTED_RETRY_MS = 2_000
const OMITTED_RETRY_LIMIT = 2
const NO_SOURCES: string[] = []

interface RatingsConfiguration {
  available: boolean
  selected: string[]
}

/**
 * Card ratings, fetched through the shared card batch scheduler. Ratings
 * start only after cards commit, so neither catalog queries nor progressive
 * first-page readiness waits on them.
 */
export function RatingsProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const settings = useSettings()
  const status = useRatingsStatus()
  const [items, setItems] = useState<ReadonlyMap<string, ItemRatings>>(new Map())
  const itemsRef = useRef<ReadonlyMap<string, ItemRatings>>(items)
  const omitted = useRef(new Map<string, number>())
  const generation = useRef(0)
  const config = useRef<RatingsConfiguration>({ available: false, selected: [] })

  const { active, pending, inFlight, batches, schedule, register: registerCard } = useCardBatches({
    enabled: () => config.current.available && config.current.selected.length > 0,
    wanted: (id) => {
      const cached = itemsRef.current.get(id)
      return !cached
        || cached.stale
        || Date.now() / 1000 - cached.fetchedAt >= CACHE_RECHECK_SECONDS
    },
    load: async (ids, signal) => {
      const currentGeneration = generation.current
      let retryingOmitted = false
      try {
        const response = await api.ratings.batch(ids, signal)
        if (currentGeneration !== generation.current) return COALESCE_MS
        if (response.items.length > 0) {
          const next = new Map(itemsRef.current)
          for (const item of response.items) next.set(item.id, item)
          itemsRef.current = next
          setItems(next)
        }
        if (response.available) {
          const returned = new Set(response.items.map((item) => item.id))
          for (const id of ids) {
            if (!active.has(id)) continue
            if (returned.has(id)) {
              omitted.current.delete(id)
              continue
            }
            const attempts = (omitted.current.get(id) ?? 0) + 1
            omitted.current.set(id, attempts)
            if (attempts <= OMITTED_RETRY_LIMIT) {
              pending.add(id)
              retryingOmitted = true
            }
          }
        }
        void queryClient.invalidateQueries({
          queryKey: queryKeys.ratingsStatus,
          refetchType: "active",
        })
      } catch (error) {
        if (error instanceof ApiError && error.accountChanged) {
          // Two accounts on one server share item ids; the next account asks
          // again for the cards it still shows.
          for (const id of ids) if (active.has(id)) pending.add(id)
        }
      }
      return retryingOmitted ? OMITTED_RETRY_MS : COALESCE_MS
    },
  })

  const selected = useMemo(
    () => settings.data?.appearance.ratingSources ?? NO_SOURCES,
    [settings.data?.appearance.ratingSources],
  )
  const available = Boolean(status.data?.selectionEnabled)
  const origin = status.data?.effectiveOrigin ?? "none"
  const configurationKey = `${available}:${origin}:${selected.join(",")}`

  useEffect(() => {
    config.current = { available, selected }
    generation.current += 1
    batches.forEach((batch) => batch.controller.abort())
    batches.clear()
    inFlight.clear()
    omitted.current.clear()
    itemsRef.current = new Map()
    setItems(itemsRef.current)
    if (available && selected.length > 0) {
      active.forEach((_count, id) => pending.add(id))
      schedule()
    }
  }, [configurationKey, available, selected, active, batches, inFlight, pending, schedule])

  useEffect(() => {
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type !== "library-changed" && detail?.type !== "catalog-changed") return
      for (const id of detail.payload.itemIds) {
        if (active.has(id)) {
          omitted.current.delete(id)
          pending.add(id)
        }
      }
      schedule()
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [active, pending, schedule])

  const register = useCallback(
    (id: string) => {
      omitted.current.delete(id)
      return registerCard(id)
    },
    [registerCard],
  )

  const definitions = useMemo(
    () => new Map((status.data?.sources ?? []).map((source) => [source.id, source])),
    [status.data?.sources],
  )
  const value = useMemo(
    () => ({ items, selected, definitions, register }),
    [items, selected, definitions, register],
  )

  return <RatingsContext.Provider value={value}>{children}</RatingsContext.Provider>
}
