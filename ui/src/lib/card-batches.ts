import { useCallback, useEffect, useRef, useState } from "react"

const UI_BATCH_SIZE = 100
const COALESCE_MS = 75

type ActiveBatch = { ids: Set<string>; controller: AbortController }

interface CardBatchOptions {
  enabled: () => boolean
  wanted: (id: string) => boolean
  load: (ids: string[], signal: AbortSignal) => Promise<void>
}

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

  const schedule = useCallback(() => {
    if (timer.current !== null || pending.size === 0) return
    timer.current = window.setTimeout(() => {
      timer.current = null
      void flushRef.current()
    }, COALESCE_MS)
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
    try {
      await latest.current.load(ids, controller.signal)
    } finally {
      batches.delete(batch)
      ids.forEach((id) => inFlight.delete(id))
      if (pending.size > 0) schedule()
    }
  }, [active, batches, inFlight, pending, schedule])

  useEffect(() => {
    flushRef.current = flush
  }, [flush])

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
