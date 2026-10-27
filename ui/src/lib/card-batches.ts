import { useCallback, useEffect, useRef, useState } from "react"

const UI_BATCH_SIZE = 100
export const COALESCE_MS = 75

type ActiveBatch = { ids: Set<string>; controller: AbortController }

interface CardBatchOptions {
  /** Whether batches may run now. */
  enabled: () => boolean
  /** Whether a card that just mounted needs its data fetched. */
  wanted: (id: string) => boolean
  /** Fetches one batch and keeps its answer; resolves to the delay before the next flush. */
  load: (ids: string[], signal: AbortSignal) => Promise<number>
}

/**
 * The scheduler behind every card's live metadata (ratings, technical
 * badges). It deduplicates the same title across shelves, coalesces arrivals
 * into bounded requests, and aborts a request once none of its cards remains
 * mounted. The options are read when used, so their identity may change freely.
 *
 * `register` stays identity-stable: cards re-run their registration effect
 * when it changes, and a whole-page cleanup pass momentarily empties `active`,
 * which reads as "no cards remain" and aborts every in-flight batch.
 */
export function useCardBatches(options: CardBatchOptions) {
  const latest = useRef(options)
  const [active] = useState(() => new Map<string, number>())
  const [pending] = useState(() => new Set<string>())
  const [inFlight] = useState(() => new Set<string>())
  const [batches] = useState(() => new Set<ActiveBatch>())
  const timer = useRef<number | null>(null)
  const flushRef = useRef<() => Promise<void>>(async () => {})

  useEffect(() => {
    latest.current = options
  })

  const schedule = useCallback((delayMs: number = COALESCE_MS) => {
    if (timer.current !== null || pending.size === 0) return
    timer.current = window.setTimeout(() => {
      timer.current = null
      void flushRef.current()
    }, delayMs)
  }, [pending])

  const flush = useCallback(async () => {
    if (!latest.current.enabled()) return
    const ids = [...pending]
      .filter((id) => active.has(id) && !inFlight.has(id))
      .slice(0, UI_BATCH_SIZE)
    ids.forEach((id) => {
      pending.delete(id)
      inFlight.add(id)
    })
    if (ids.length === 0) return

    const controller = new AbortController()
    const batch: ActiveBatch = { ids: new Set(ids), controller }
    batches.add(batch)
    let delayMs = COALESCE_MS
    try {
      delayMs = await latest.current.load(ids, controller.signal)
    } finally {
      batches.delete(batch)
      ids.forEach((id) => inFlight.delete(id))
      if (pending.size > 0) schedule(delayMs)
    }
  }, [active, batches, inFlight, pending, schedule])

  useEffect(() => {
    flushRef.current = flush
  }, [flush])

  /** Drops queued work and aborts every request in flight. */
  const cancel = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current)
      timer.current = null
    }
    pending.clear()
    batches.forEach((batch) => batch.controller.abort())
  }, [batches, pending])

  useEffect(() => cancel, [cancel])

  const register = useCallback(
    (id: string) => {
      active.set(id, (active.get(id) ?? 0) + 1)
      if (latest.current.enabled() && latest.current.wanted(id)) {
        pending.add(id)
        schedule()
      }
      return () => {
        const count = (active.get(id) ?? 1) - 1
        if (count > 0) {
          active.set(id, count)
          return
        }
        active.delete(id)
        pending.delete(id)
        for (const batch of batches) {
          if (!batch.ids.has(id)) continue
          const anyActive = [...batch.ids].some((batchId) => active.has(batchId))
          if (!anyActive) batch.controller.abort()
        }
      }
    },
    [active, batches, pending, schedule],
  )

  return { active, pending, inFlight, batches, schedule, cancel, register }
}
