import { useQueryClient } from "@tanstack/react-query"
import { useEffect, useState } from "react"
import { toast } from "sonner"
import { api, ApiError } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import type { ReadingPosition } from "./open-book"

const SAVE_INTERVAL_MS = 1000

/**
 * The open book's one writer of its place. The newest position waits up to a
 * second, one save runs at a time, reaching the end saves at once, and the
 * last position is written when the reader closes or the page is hidden.
 * Each save also refreshes the book's detail answer, so Continue shows it.
 */
export function useProgress(itemId: string): (position: ReadingPosition) => void {
  const queryClient = useQueryClient()
  const [pump] = useState(() => {
    let pending: ReadingPosition | null = null
    let running = false
    let stopped = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const save = async () => {
      clearTimeout(timer)
      timer = undefined
      if (running || stopped || !pending) return
      const position = pending
      pending = null
      running = true
      try {
        await api.saveBookPosition(itemId, { location: position.location, finished: position.finished })
        void queryClient.invalidateQueries({ queryKey: queryKeys.item(itemId) })
      } catch (error) {
        stopped = error instanceof ApiError && (error.accountChanged || error.expired)
        if (!stopped) toast.error("Could not keep your place in this book", { id: `book-place-${itemId}` })
      } finally {
        running = false
        if (pending && !stopped) timer = setTimeout(save, SAVE_INTERVAL_MS)
      }
    }
    return {
      report(position: ReadingPosition) {
        pending = position
        if (position.finished) void save()
        else timer ??= setTimeout(save, SAVE_INTERVAL_MS)
      },
      flush: () => void save(),
    }
  })

  useEffect(() => {
    window.addEventListener("pagehide", pump.flush)
    return () => {
      window.removeEventListener("pagehide", pump.flush)
      pump.flush()
    }
  }, [pump])

  return pump.report
}
