import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { api, type MediaStream } from "./api"
import { useSettings } from "./queries"
import { readShellEvent, shellEventIds } from "./shell-events"
import { TechnicalContext } from "./technical-context"

const UI_BATCH_SIZE = 100
const COALESCE_MS = 75
/** Live stream metadata is refreshed even when a remux leaves the catalog row unchanged. */
const TECHNICAL_STALE_MS = 15 * 60_000
const STALE_SCAN_MS = 60_000

type ActiveBatch = { ids: Set<string>; controller: AbortController }

function isAbortError(cause: unknown) {
  return cause instanceof DOMException && cause.name === "AbortError"
}

/**
 * One scheduler for every visible card's quality badges, modeled on the
 * ratings scheduler next door: it deduplicates the same title across shelves,
 * coalesces arrivals into bounded live batches, and aborts a request once none
 * of its cards remains visible. Streams are never cached natively — this map
 * is their only memory. Successful and failed lookups are rechecked on a
 * modest timer so an in-place remux cannot leave stale badges for the session.
 */
export function TechnicalProvider({ children }: { children: ReactNode }) {
  const settings = useSettings()
  const [items, setItems] = useState<ReadonlyMap<string, MediaStream[]>>(new Map())
  const checkedAt = useRef(new Map<string, number>())
  const active = useRef(new Map<string, number>())
  const pending = useRef(new Set<string>())
  const inFlight = useRef(new Set<string>())
  const batches = useRef(new Set<ActiveBatch>())
  const timer = useRef<number | null>(null)
  const enabled = useRef(false)
  const flushRef = useRef<() => Promise<void>>(async () => {})

  const fresh = useCallback((id: string) => {
    const checked = checkedAt.current.get(id)
    return checked !== undefined && Date.now() - checked < TECHNICAL_STALE_MS
  }, [])

  const schedule = useCallback(() => {
    if (timer.current !== null || pending.current.size === 0) return
    timer.current = window.setTimeout(() => {
      timer.current = null
      void flushRef.current()
    }, COALESCE_MS)
  }, [])

  flushRef.current = async () => {
    if (!enabled.current) return
    const ids = [...pending.current]
      .filter((id) => active.current.has(id) && !inFlight.current.has(id))
      .slice(0, UI_BATCH_SIZE)
    ids.forEach((id) => {
      pending.current.delete(id)
      inFlight.current.add(id)
    })
    if (ids.length === 0) return

    const controller = new AbortController()
    const batch: ActiveBatch = { ids: new Set(ids), controller }
    batches.current.add(batch)
    try {
      const response = await api.technical.batch(ids, controller.signal)
      if (controller.signal.aborted) return
      const returned = new Map(response.items.map((item) => [item.id, item.mediaStreams]))
      // A successful lookup that has no representative stream is a real empty
      // result. Remember it too, or remounting that card would query forever.
      setItems((current) => {
        const next = new Map(current)
        for (const id of ids) next.set(id, returned.get(id) ?? [])
        return next
      })
      const now = Date.now()
      for (const id of ids) checkedAt.current.set(id, now)
    } catch (error) {
      if (!isAbortError(error)) {
        // Failed/offline cards stay quiet and wait for the same bounded stale
        // interval before retrying instead of creating a request loop.
        const now = Date.now()
        for (const id of ids) checkedAt.current.set(id, now)
      }
    } finally {
      batches.current.delete(batch)
      ids.forEach((id) => inFlight.current.delete(id))
      if (pending.current.size > 0) schedule()
    }
  }

  // Unknown settings are disabled: otherwise a saved false value can lose a
  // race with the initial settings request and still send a technical batch.
  const showMediaInfo = settings.data?.appearance.showMediaInfo === true

  useEffect(() => {
    enabled.current = showMediaInfo
    if (showMediaInfo) {
      active.current.forEach((_count, id) => {
        if (!fresh(id)) pending.current.add(id)
      })
      schedule()
      return
    }
    if (timer.current !== null) {
      window.clearTimeout(timer.current)
      timer.current = null
    }
    pending.current.clear()
    batches.current.forEach((batch) => batch.controller.abort())
  }, [fresh, showMediaInfo, schedule])

  // MediaStreams are deliberately absent from the catalog index, so an
  // in-place remux can leave every catalog column unchanged. Refresh only the
  // cards still visible, at a low cadence, to discover that change without
  // restoring a durable rich-metadata cache.
  useEffect(() => {
    const interval = window.setInterval(() => {
      if (!enabled.current) return
      active.current.forEach((_count, id) => {
        if (!fresh(id)) pending.current.add(id)
      })
      schedule()
    }, STALE_SCAN_MS)
    return () => window.clearInterval(interval)
  }, [fresh, schedule])

  // Changed episodes affect the representative stream of their series and
  // season cards, so both direct item ids and parent context ids invalidate the
  // live map. Off-screen entries are dropped now and fetched only if visible
  // again; visible entries are requeued immediately.
  useEffect(() => {
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type !== "library-changed") return
      const changed = [...new Set([
        ...shellEventIds(detail.payload.itemIds),
        ...shellEventIds(detail.payload.contextIds),
      ])]
      if (changed.length === 0) return

      for (const id of changed) checkedAt.current.delete(id)
      setItems((current) => {
        if (!changed.some((id) => current.has(id))) return current
        const next = new Map(current)
        for (const id of changed) next.delete(id)
        return next
      })
      for (const batch of batches.current) {
        if (!changed.some((id) => batch.ids.has(id))) continue
        batch.controller.abort()
        for (const id of batch.ids) {
          if (active.current.has(id)) pending.current.add(id)
        }
      }
      for (const id of changed) {
        if (active.current.has(id)) pending.current.add(id)
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
      if (enabled.current && !fresh(id)) {
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
    [fresh, schedule],
  )

  const value = useMemo(() => ({ items, register }), [items, register])

  return <TechnicalContext.Provider value={value}>{children}</TechnicalContext.Provider>
}
