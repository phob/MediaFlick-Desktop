import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { api, type ItemRatings } from "./api"
import { RatingsContext } from "./rating-context"
import { queryClient, queryKeys } from "./query-client"
import { useRatingsStatus, useSettings } from "./queries"
import { readShellEvent, shellEventIds } from "./shell-events"

const UI_BATCH_SIZE = 100
const COALESCE_MS = 75
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

type ActiveBatch = { ids: Set<string>; controller: AbortController }

interface RatingsConfiguration {
  available: boolean
  selected: string[]
}

/**
 * One scheduler for every mounted card. It deduplicates the same title across
 * shelves, coalesces arrivals into bounded requests, and aborts a request once
 * none of its cards remains mounted. Ratings start only after cards commit, so
 * neither catalog queries nor progressive first-page readiness waits on them.
 *
 * `register` must stay identity-stable like the technical scheduler's: cards
 * re-run their registration effect when it changes, and a whole-page cleanup
 * pass momentarily empties `active`, which reads as "no cards remain" and
 * aborts every other in-flight batch mid-request.
 */
export function RatingsProvider({ children }: { children: ReactNode }) {
  const settings = useSettings()
  const status = useRatingsStatus()
  const [items, setItems] = useState<ReadonlyMap<string, ItemRatings>>(new Map())
  const itemsRef = useRef<ReadonlyMap<string, ItemRatings>>(items)
  const active = useRef(new Map<string, number>())
  const pending = useRef(new Set<string>())
  const inFlight = useRef(new Set<string>())
  const omitted = useRef(new Map<string, number>())
  const batches = useRef(new Set<ActiveBatch>())
  const timer = useRef<number | null>(null)
  const generation = useRef(0)
  const config = useRef<RatingsConfiguration>({ available: false, selected: [] })
  const flushRef = useRef<() => Promise<void>>(async () => {})

  const schedule = useCallback((delayMs: number = COALESCE_MS) => {
    if (timer.current !== null || pending.current.size === 0) return
    timer.current = window.setTimeout(() => {
      timer.current = null
      void flushRef.current()
    }, delayMs)
  }, [])

  const flush = useCallback(async () => {
    if (!config.current.available || config.current.selected.length === 0) return
    const ids = [...pending.current]
      .filter((id) => active.current.has(id) && !inFlight.current.has(id))
      .slice(0, UI_BATCH_SIZE)
    ids.forEach((id) => {
      pending.current.delete(id)
      inFlight.current.add(id)
    })
    if (ids.length === 0) return

    const currentGeneration = generation.current
    const controller = new AbortController()
    const batch: ActiveBatch = { ids: new Set(ids), controller }
    batches.current.add(batch)
    let retryingOmitted = false
    try {
      const response = await api.ratings.batch(ids, controller.signal)
      if (currentGeneration !== generation.current) return
      if (response.items.length > 0) {
        const next = new Map(itemsRef.current)
        for (const item of response.items) next.set(item.id, item)
        itemsRef.current = next
        setItems(next)
      }
      if (response.available) {
        const returned = new Set(response.items.map((item) => item.id))
        for (const id of ids) {
          if (!active.current.has(id)) continue
          if (returned.has(id)) {
            omitted.current.delete(id)
            continue
          }
          const attempts = (omitted.current.get(id) ?? 0) + 1
          omitted.current.set(id, attempts)
          if (attempts <= OMITTED_RETRY_LIMIT) {
            pending.current.add(id)
            retryingOmitted = true
          }
        }
      }
      // Quota, 401 and Retry-After state is canonical in the native service.
      // Refetch only if the status view is active; cards do not need another
      // network request to display the response they just received.
      void queryClient.invalidateQueries({
        queryKey: queryKeys.ratingsStatus,
        refetchType: "active",
      })
    } catch (error) {
      if (!(error instanceof DOMException && error.name === "AbortError")) {
        // A missing/offline overlay is intentionally silent. The integration
        // page exposes the actionable validation/backoff detail.
      }
    } finally {
      batches.current.delete(batch)
      ids.forEach((id) => inFlight.current.delete(id))
      if (pending.current.size > 0) schedule(retryingOmitted ? OMITTED_RETRY_MS : COALESCE_MS)
    }
  }, [schedule])

  useEffect(() => {
    flushRef.current = flush
  }, [flush])

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
    batches.current.forEach((batch) => batch.controller.abort())
    batches.current.clear()
    inFlight.current.clear()
    omitted.current.clear()
    itemsRef.current = new Map()
    setItems(itemsRef.current)
    if (available && selected.length > 0) {
      active.current.forEach((_count, id) => pending.current.add(id))
      schedule()
    }
  }, [configurationKey, available, selected, schedule])

  useEffect(() => {
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type !== "library-changed") return
      for (const id of shellEventIds(detail.payload.itemIds)) {
        if (active.current.has(id)) {
          omitted.current.delete(id)
          pending.current.add(id)
        }
      }
      schedule()
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [schedule])

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current)
      batches.current.forEach((batch) => batch.controller.abort())
    },
    [],
  )

  const register = useCallback(
    (id: string) => {
      active.current.set(id, (active.current.get(id) ?? 0) + 1)
      // A remount is a fresh look at the card, so it earns fresh retries.
      omitted.current.delete(id)
      const cached = itemsRef.current.get(id)
      const shouldRecheck = !cached
        || cached.stale
        || Date.now() / 1000 - cached.fetchedAt >= CACHE_RECHECK_SECONDS
      if (config.current.available && config.current.selected.length > 0 && shouldRecheck) {
        pending.current.add(id)
        schedule()
      }
      return () => {
        const count = (active.current.get(id) ?? 1) - 1
        if (count > 0) {
          active.current.set(id, count)
          return
        }
        active.current.delete(id)
        pending.current.delete(id)
        for (const batch of batches.current) {
          if (!batch.ids.has(id)) continue
          const anyActive = [...batch.ids].some((batchId) => active.current.has(batchId))
          if (!anyActive) batch.controller.abort()
        }
      }
    },
    [schedule],
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
