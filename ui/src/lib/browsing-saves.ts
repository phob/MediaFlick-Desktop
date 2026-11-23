import { api } from "./api/client.ts"
import type { BrowsingPage } from "./api/types/libraries.ts"

export type LatestSaveEntry<T> = {
  value: T
  onSettled?: () => void
  onError?: (error: Error) => void
}

export function createLatestSaveQueue<T>(save: (value: T) => Promise<void>) {
  let active = false
  let pending: LatestSaveEntry<T> | null = null

  const drain = async () => {
    if (active) return
    active = true
    try {
      while (pending !== null) {
        const entry = pending
        pending = null
        try {
          await save(entry.value)
          if (pending === null) entry.onSettled?.()
        } catch (caught) {
          if (pending === null) {
            const error = caught instanceof Error ? caught : new Error("Could not save change")
            entry.onError?.(error)
          }
        }
      }
    } finally {
      active = false
    }
  }

  return (entry: LatestSaveEntry<T>) => {
    pending = entry
    void drain()
  }
}

const queues = new Map<string, (entry: LatestSaveEntry<string>) => void>()

export function saveBrowsingOrdered(
  account: string,
  page: BrowsingPage,
  route: string,
  callbacks: { onSettled?: () => void; onError?: (error: Error) => void } = {},
) {
  const key = `${account}\0${page}`
  let enqueue = queues.get(key)
  if (!enqueue) {
    enqueue = createLatestSaveQueue<string>(async (nextRoute) => {
      await api.saveBrowsing(page, nextRoute, account)
    })
    queues.set(key, enqueue)
  }
  enqueue({ value: route, ...callbacks })
}
