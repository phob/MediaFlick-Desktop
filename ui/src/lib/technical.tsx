import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { api, ApiError, type MediaStream } from "./api"
import { COALESCE_MS, useCardBatches } from "./card-batches"
import { useSettings } from "./queries"
import { readShellEvent } from "./shell-events"
import { TechnicalContext } from "./technical-context"

/** Live stream metadata is refreshed even when a remux leaves the catalog row unchanged. */
const TECHNICAL_STALE_MS = 15 * 60_000
const STALE_SCAN_MS = 60_000

function isAbortError(cause: unknown) {
  return cause instanceof DOMException && cause.name === "AbortError"
}

/**
 * Every visible card's quality badges, fetched through the shared card batch
 * scheduler. Streams are never cached natively — this map is their only
 * memory. Successful and failed lookups are rechecked on a modest timer so an
 * in-place remux cannot leave stale badges for the session.
 */
export function TechnicalProvider({ children }: { children: ReactNode }) {
  const settings = useSettings()
  const [items, setItems] = useState<ReadonlyMap<string, MediaStream[]>>(new Map())
  const checkedAt = useRef(new Map<string, number>())
  const enabled = useRef(false)

  const fresh = useCallback((id: string) => {
    const checked = checkedAt.current.get(id)
    return checked !== undefined && Date.now() - checked < TECHNICAL_STALE_MS
  }, [])

  const markChecked = useCallback((ids: string[]) => {
    const now = Date.now()
    for (const id of ids) checkedAt.current.set(id, now)
  }, [])

  const { active, pending, batches, schedule, cancel, register } = useCardBatches({
    enabled: () => enabled.current,
    wanted: (id) => !fresh(id),
    load: async (ids, signal) => {
      try {
        const response = await api.technical.batch(ids, signal)
        if (signal.aborted) return COALESCE_MS
        const returned = new Map(response.items.map((item) => [item.id, item.mediaStreams]))
        setItems((current) => {
          const next = new Map(current)
          for (const id of ids) next.set(id, returned.get(id) ?? [])
          return next
        })
        markChecked(ids)
      } catch (error) {
        if (error instanceof ApiError && error.accountChanged) {
          // Two accounts on one server share item ids, so the previous
          // account's failure must not stand in for the next one's lookup.
          for (const id of ids) if (active.has(id)) pending.add(id)
        } else if (!isAbortError(error)) {
          markChecked(ids)
        }
      }
      return COALESCE_MS
    },
  })

  const showMediaInfo = settings.data?.appearance.showMediaInfo === true

  useEffect(() => {
    enabled.current = showMediaInfo
    if (showMediaInfo) {
      active.forEach((_count, id) => {
        if (!fresh(id)) pending.add(id)
      })
      schedule()
      return
    }
    cancel()
  }, [active, cancel, fresh, pending, showMediaInfo, schedule])

  // MediaStreams are deliberately absent from the catalog index, so an
  // in-place remux can leave every catalog column unchanged. Refresh only the
  // cards still visible, at a low cadence, to discover that change without
  // restoring a durable rich-metadata cache.
  useEffect(() => {
    const interval = window.setInterval(() => {
      if (!enabled.current) return
      active.forEach((_count, id) => {
        if (!fresh(id)) pending.add(id)
      })
      schedule()
    }, STALE_SCAN_MS)
    return () => window.clearInterval(interval)
  }, [active, fresh, pending, schedule])

  // Changed episodes affect the representative stream of their series and
  // season cards, so both direct item ids and parent context ids invalidate the
  // live map. Off-screen entries are dropped now and fetched only if visible
  // again; visible entries are requeued immediately.
  useEffect(() => {
    const receive = (event: Event) => {
      const detail = readShellEvent(event)
      if (detail?.type !== "library-changed" && detail?.type !== "catalog-changed") return
      const changed = [...new Set([
        ...detail.payload.itemIds,
        ...detail.payload.contextIds,
      ])]
      if (changed.length === 0) return

      for (const id of changed) checkedAt.current.delete(id)
      setItems((current) => {
        if (!changed.some((id) => current.has(id))) return current
        const next = new Map(current)
        for (const id of changed) next.delete(id)
        return next
      })
      for (const batch of batches) {
        if (!changed.some((id) => batch.ids.has(id))) continue
        batch.controller.abort()
        for (const id of batch.ids) {
          if (active.has(id)) pending.add(id)
        }
      }
      for (const id of changed) {
        if (active.has(id)) pending.add(id)
      }
      schedule()
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [active, batches, pending, schedule])

  const value = useMemo(() => ({ items, register }), [items, register])

  return <TechnicalContext.Provider value={value}>{children}</TechnicalContext.Provider>
}
