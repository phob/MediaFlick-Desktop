import { useQueryClient } from "@tanstack/react-query"
import { useEffect, useRef } from "react"
import { toast } from "sonner"
import { api, ApiError } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import type { ReadingPosition } from "./open-book"

const SAVE_INTERVAL_MS = 1000

/**
 * The one writer of a book's place for an account, shared by every reader of
 * that book, closed ones included. The newest position waits up to a second,
 * one save runs at a time, reaching the end saves at once, and what is left
 * is written as soon as the reader closes or the page is hidden. A reader's
 * reports stop counting once it closes or a newer reader of the book opens.
 */
interface Writer {
  itemId: string
  /** The newest reader's session; only its reports count. */
  session: number
  /** The newest reader is still mounted. */
  open: boolean
  pending: ReadingPosition | null
  timer: ReturnType<typeof setTimeout> | undefined
  saving: Promise<void> | null
  /** The account changed or its session expired: nothing more is written. */
  stopped: boolean
  saved(): void
}

const writers = new Map<string, Writer>()
let sessions = 0

const writerKey = (account: string, itemId: string) => `${account}\n${itemId}`

async function save(key: string, writer: Writer): Promise<void> {
  clearTimeout(writer.timer)
  writer.timer = undefined
  if (!writer.saving && !writer.stopped && writer.pending) {
    const { location, finished } = writer.pending
    writer.pending = null
    writer.saving = api.saveBookPosition(writer.itemId, { location, finished }).then(
      () => writer.saved(),
      (error: unknown) => {
        writer.stopped = error instanceof ApiError && (error.accountChanged || error.expired)
        if (!writer.stopped) toast.error("Could not keep your place in this book", { id: `book-place-${writer.itemId}` })
      },
    )
    await writer.saving
    writer.saving = null
    if (writer.pending && !writer.stopped) {
      if (writer.open) writer.timer = setTimeout(() => void save(key, writer), SAVE_INTERVAL_MS)
      else return save(key, writer)
    }
  }
  if (!writer.open && !writer.saving && (writer.stopped || !writer.pending) && writers.get(key) === writer) writers.delete(key)
}

/** Resolves once nothing is left to save for the book, so a reader opening it reads the place last kept. */
export async function settled(account: string, itemId: string): Promise<void> {
  const key = writerKey(account, itemId)
  for (let writer = writers.get(key); writer && !writer.stopped && (writer.saving || writer.pending); writer = writers.get(key)) {
    await (writer.saving ?? save(key, writer))
  }
}

/** Starts a reader's session with the book's writer and returns how it reports positions. */
export function useProgress(account: string, itemId: string): (position: ReadingPosition) => void {
  const queryClient = useQueryClient()
  const session = useRef(0)

  useEffect(() => {
    const key = writerKey(account, itemId)
    let writer = writers.get(key)
    if (!writer) {
      writer = { itemId, session: 0, open: false, pending: null, timer: undefined, saving: null, stopped: false, saved: () => undefined }
      writers.set(key, writer)
    }
    const mine = ++sessions
    writer.session = mine
    writer.open = true
    writer.stopped = false
    writer.saved = () => void queryClient.invalidateQueries({ queryKey: queryKeys.item(itemId) })
    session.current = mine
    const flush = () => void save(key, writer)
    window.addEventListener("pagehide", flush)
    return () => {
      window.removeEventListener("pagehide", flush)
      if (writer.session === mine) writer.open = false
      flush()
    }
  }, [account, itemId, queryClient])

  return (position) => {
    const key = writerKey(account, itemId)
    const writer = writers.get(key)
    if (!writer?.open || writer.session !== session.current) return
    writer.pending = position
    if (position.finished) void save(key, writer)
    else writer.timer ??= setTimeout(() => void save(key, writer), SAVE_INTERVAL_MS)
  }
}
