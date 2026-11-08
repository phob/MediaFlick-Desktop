import { useQuery } from "@tanstack/react-query"
import { useEffect, useEffectEvent, useRef, useState } from "react"
import { useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom"
import { toast } from "sonner"
import { Menu } from "lucide-react"
import { Button } from "@/components/ui/button"
import { api, type BookManifest, type ReaderAppearance } from "@/lib/api"
import { queryKeys } from "@/lib/query-client"
import { FONT_SIZE } from "./appearance"
import { commandFor, type KeyContext } from "./keys"
import { nextLayer, type ReaderCommand, type ReaderLayer } from "./model"
import { openBook, type BookSession, type JumpTarget, type ReadingPosition } from "./open-book"
import { ReaderPanels, type PanelControl } from "./panels"
import { useProgress } from "./progress"
import type { ReaderEntry } from "./route"

type Phase = { kind: "opening"; downloaded: number | null } | { kind: "ready"; session: BookSession } | { kind: "failed"; message: string }

function useLeaveReader(itemId: string) {
  const navigate = useNavigate()
  const location = useLocation()
  return () => (location.key === "default" ? navigate(`/item/${encodeURIComponent(itemId)}`, { replace: true }) : navigate(-1))
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export default function Reader() {
  const { id = "" } = useParams()
  const [search] = useSearchParams()
  const entry: ReaderEntry = search.get("from") === "start" ? "start" : "resume"
  const manifest = useQuery({
    queryKey: queryKeys.book(id, entry),
    queryFn: ({ signal }) => api.book(id, entry, signal),
    staleTime: 0,
    gcTime: 0,
    retry: false,
  })
  return <BookReader key={`${id}:${entry}`} itemId={id} manifest={manifest.data} failure={manifest.error ? errorMessage(manifest.error) : null} />
}

function BookReader({ itemId, manifest, failure }: { itemId: string; manifest: BookManifest | undefined; failure: string | null }) {
  const host = useRef<HTMLDivElement>(null)
  const leave = useLeaveReader(itemId)
  const report = useProgress(itemId)
  const [phase, setPhase] = useState<Phase>({ kind: "opening", downloaded: null })
  const [layer, setLayer] = useState<ReaderLayer>({ kind: "reading" })
  const [position, setPosition] = useState<ReadingPosition | null>(null)
  const [appearance, setAppearance] = useState<ReaderAppearance | null>(null)
  const saving = useRef(Promise.resolve())
  const panel = useRef<PanelControl | null>(null)
  const session = phase.kind === "ready" ? phase.session : null
  const shownAppearance = appearance ?? manifest?.appearance ?? null

  const changeAppearance = (next: ReaderAppearance) => {
    setAppearance(next)
    session?.apply(next)
    saving.current = saving.current.then(() =>
      api.saveReaderAppearance(next).then(
        () => undefined,
        () => void toast.error("Could not keep the reader's appearance", { id: "reader-appearance" }),
      ),
    )
  }

  const run = (command: ReaderCommand) => {
    switch (command.kind) {
      case "back":
      case "open": {
        const next = nextLayer(layer, command)
        if (next === "leave") leave()
        else setLayer(next)
        return
      }
      case "font": {
        if (!shownAppearance) return
        const fontSize = Math.min(FONT_SIZE.max, Math.max(FONT_SIZE.min, shownAppearance.fontSize + command.step * FONT_SIZE.step))
        changeAppearance({ ...shownAppearance, fontSize })
        return
      }
      case "move":
        return panel.current?.move(command.to)
      case "confirm":
        return panel.current?.confirm()
      case "adjust":
        return panel.current?.adjust(command.by)
      default:
        void session?.run(command)
    }
  }

  const jump = (target: JumpTarget) => {
    setLayer({ kind: "reading" })
    void session?.jump(target)
  }

  useEffect(() => {
    if (layer.kind === "reading" && session) host.current?.focus()
  }, [layer.kind, session])

  const keyContext: KeyContext = { flow: shownAppearance?.flow ?? "paginated", reflowable: session?.layout !== "fixed" }

  const onKey = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented) return
    const command = commandFor(layer.kind, event, keyContext)
    if (!command || (!session && command.kind !== "back")) return
    event.preventDefault()
    event.stopPropagation()
    run(command)
  })

  useEffect(() => {
    const listener = (event: KeyboardEvent) => onKey(event)
    window.addEventListener("keydown", listener, true)
    return () => window.removeEventListener("keydown", listener, true)
  }, [])

  const onPosition = useEffectEvent((next: ReadingPosition) => {
    setPosition(next)
    report(next)
  })
  const onDownload = useEffectEvent((downloaded: number) => setPhase({ kind: "opening", downloaded }))

  useEffect(() => {
    const element = host.current
    if (!element || !manifest) return
    const controller = new AbortController()
    let opened: BookSession | null = null
    openBook(element, itemId, manifest, {
      signal: controller.signal,
      onKey: (event) => onKey(event),
      onPosition: (next) => onPosition(next),
      onDownload: (fraction) => onDownload(fraction),
    }).then(
      (book) => {
        opened = book
        const current = book.position()
        if (current) onPosition(current)
        setPhase({ kind: "ready", session: book })
        element.focus()
      },
      (error: unknown) => {
        if (!controller.signal.aborted) setPhase({ kind: "failed", message: errorMessage(error) })
      },
    )
    return () => {
      controller.abort()
      opened?.close()
      element.replaceChildren()
    }
  }, [itemId, manifest])

  const shownFailure = failure ?? (phase.kind === "failed" ? phase.message : null)
  return (
    <div className="relative flex h-full flex-col" data-reader-book={itemId} data-reader-phase={shownFailure ? "failed" : phase.kind} data-reader-layer={layer.kind}>
      <div ref={host} tabIndex={-1} aria-label={manifest?.title ?? "Book"} className="min-h-0 flex-1 outline-none" />
      {shownFailure ? (
        <ReaderMessage title="This book could not be opened" detail={shownFailure} onLeave={leave} />
      ) : phase.kind === "opening" ? (
        <ReaderMessage title="Opening the book" detail={phase.downloaded === null ? null : `Downloaded ${Math.round(phase.downloaded * 100)}%`} />
      ) : null}
      <ReaderFooter position={position} onMenu={session ? () => run({ kind: "open", layer: "menu" }) : undefined} />
      {session && shownAppearance && (
        <ReaderPanels
          layer={layer}
          control={panel}
          layout={session.layout}
          toc={session.toc}
          position={position}
          appearance={shownAppearance}
          keyContext={keyContext}
          run={run}
          jump={jump}
          changeAppearance={changeAppearance}
          leave={leave}
        />
      )}
    </div>
  )
}

function ReaderMessage({ title, detail, onLeave }: { title: string; detail: string | null; onLeave?: () => void }) {
  return (
    <div role={onLeave ? "alert" : "status"} className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-background p-8 text-center">
      <p className="text-lg font-semibold">{title}</p>
      {detail && <p className="max-w-xl text-sm text-muted-foreground">{detail}</p>}
      {onLeave && (
        <Button autoFocus variant="outline" onClick={onLeave}>
          Back
        </Button>
      )}
    </div>
  )
}

function ReaderFooter({ position, onMenu }: { position: ReadingPosition | null; onMenu: (() => void) | undefined }) {
  const count = position?.page
    ? `Page ${position.page.current + 1} of ${position.page.total}`
    : position?.textLocation
      ? `Location ${position.textLocation.current + 1} of ${position.textLocation.total}`
      : null
  return (
    <div className="flex h-8 shrink-0 items-center gap-3 px-2 text-xs text-muted-foreground" data-reader-footer>
      {onMenu && (
        <Button variant="ghost" size="sm" className="h-7 px-2 text-xs" tabIndex={-1} onClick={onMenu}>
          <Menu aria-hidden />
          Menu
        </Button>
      )}
      <span className="min-w-0 flex-1 truncate">{position?.chapter}</span>
      {position && <span className="shrink-0">{[count, `${Math.round(position.fraction * 100)}%`].filter(Boolean).join(" · ")}</span>}
    </div>
  )
}
