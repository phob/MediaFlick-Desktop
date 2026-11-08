import { useEffect, useRef, useState } from "react"
import { useParams, useSearchParams } from "react-router-dom"
import { openBook, type BookFormat } from "./open-book"

const FORMATS: readonly BookFormat[] = ["epub", "mobi", "azw", "azw3", "pdf", "cbz", "cbr", "cb7", "cbt"]

type ReaderState = { phase: "loading"; downloaded: number | null } | { phase: "ready" } | { phase: "failed"; message: string }

export default function Reader() {
  const { id = "" } = useParams()
  const [search] = useSearchParams()
  const format = FORMATS.find((candidate) => candidate === search.get("format"))
  return <OpenBook key={`${id}.${format}`} id={id} format={format} />
}

function OpenBook({ id, format }: { id: string; format: BookFormat | undefined }) {
  const host = useRef<HTMLDivElement>(null)
  const [state, setState] = useState<ReaderState>({ phase: "loading", downloaded: null })
  const [label, setLabel] = useState("")

  useEffect(() => {
    const element = host.current
    if (!element || !format) return
    const controller = new AbortController()
    let close = () => {}
    let turn: (side: "left" | "right") => void = () => {}
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return
      event.preventDefault()
      turn(event.key === "ArrowLeft" ? "left" : "right")
    }
    window.addEventListener("keydown", onKey, { capture: true, signal: controller.signal })
    openBook(element, id, format, {
      signal: controller.signal,
      onDocument: (doc) => doc.addEventListener("keydown", onKey, { capture: true, signal: controller.signal }),
      onRelocate: (location) =>
        setLabel(
          `${(location.fraction * 100).toFixed(1)}% · location ${location.location.current + 1}/${location.location.total} · section ${location.section.current + 1}/${location.section.total}`,
        ),
      onDownload: (fraction) => setState({ phase: "loading", downloaded: fraction }),
    }).then(
      (view) => {
        close = () => view.close()
        turn = (side) => void (side === "left" ? view.goLeft() : view.goRight())
        if (controller.signal.aborted) close()
        else setState({ phase: "ready" })
      },
      (error: unknown) => {
        if (!controller.signal.aborted) setState({ phase: "failed", message: error instanceof Error ? error.message : String(error) })
      },
    )
    return () => {
      controller.abort()
      close()
      element.replaceChildren()
    }
  }, [id, format])

  return (
    <div className="flex h-full flex-col bg-background">
      <div ref={host} className="min-h-0 flex-1" data-reader-book={id} data-reader-phase={state.phase} />
      <p role="status" className="px-4 py-2 text-sm text-muted-foreground">
        {!format ? "Unknown book format" : state.phase === "ready" ? label : state.phase === "failed" ? state.message : "Opening…"}
      </p>
    </div>
  )
}
