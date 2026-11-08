import { useEffect, useImperativeHandle, useRef, useState, type ReactNode, type Ref } from "react"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Slider } from "@/components/ui/slider"
import type { ReaderAppearance } from "@/lib/api"
import { cn } from "@/lib/utils"
import { FONT_SIZE } from "./appearance"
import { bindingsFor, type KeyContext } from "./keys"
import type { MenuEntry, ReaderCommand, ReaderLayer } from "./model"
import type { JumpTarget, ReadingPosition, TocEntry } from "./open-book"

type Move = Extract<ReaderCommand, { kind: "move" }>["to"]

/** What the reader's keys ask of the open panel. */
export interface PanelControl {
  move(to: Move): void
  confirm(): void
  adjust(by: -1 | 1): void
}

const ROW = "[data-reader-row]"

/** Focuses a row of `container`: the next or previous one, ten on, or an end. */
function moveFocus(container: HTMLElement | null, to: Move) {
  const rows = [...(container?.querySelectorAll<HTMLElement>(ROW) ?? [])]
  if (!rows.length) return
  const current = rows.findIndex((row) => row.contains(document.activeElement))
  const step = { previous: -1, next: 1, previousTen: -10, nextTen: 10, first: -Infinity, last: Infinity }[to]
  const index = Math.min(rows.length - 1, Math.max(0, (current < 0 ? 0 : current) + step))
  focusRow(rows[index])
}

/** A row's own control takes the focus: a slider's thumb, the chosen radio, or the row itself. */
function focusRow(row: HTMLElement | undefined) {
  const target =
    row?.querySelector<HTMLElement>('[role="slider"], [role="radio"][data-state="checked"], [role="radio"]') ?? row
  target?.focus()
  row?.scrollIntoView({ block: "nearest" })
}

interface PanelsProps {
  layer: ReaderLayer
  control: Ref<PanelControl | null>
  layout: "reflowable" | "fixed"
  toc: readonly TocEntry[]
  position: ReadingPosition | null
  appearance: ReaderAppearance
  keyContext: KeyContext
  run(command: ReaderCommand): void
  jump(target: JumpTarget): void
  changeAppearance(next: ReaderAppearance): void
  leave(): void
}

const TITLES: Record<Exclude<ReaderLayer["kind"], "reading">, string> = {
  menu: "Menu",
  toc: "Contents",
  goto: "Go to",
  settings: "Appearance",
  keys: "Keys",
}

/**
 * The reader's panels, in one sheet whose content follows the layer, so
 * moving from the menu to a panel never closes one dialog while another
 * opens. The reader's key table owns Back and the moves between rows; a
 * row's own control keeps Enter, and the sliders and radio groups keep
 * Left and Right, so Back always closes exactly one panel.
 */
export function ReaderPanels(props: PanelsProps) {
  const { layer } = props
  const body = useRef<HTMLDivElement>(null)
  const goTo = useRef<{ confirm(): void } | null>(null)
  const settings = useRef<{ adjust(by: -1 | 1): void } | null>(null)
  useImperativeHandle(
    props.control,
    () => ({
      move: (to) => moveFocus(body.current, to),
      confirm: () => goTo.current?.confirm(),
      adjust: (by) => settings.current?.adjust(by),
    }),
    [],
  )
  return (
    <Sheet open={layer.kind !== "reading"} onOpenChange={(open) => !open && props.run({ kind: "back" })}>
      <SheetContent
        side="left"
        className="w-full gap-0 sm:max-w-md"
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
        data-reader-panel={layer.kind}
      >
        {layer.kind !== "reading" && (
          <SheetHeader>
            <SheetTitle>{TITLES[layer.kind]}</SheetTitle>
            <SheetDescription className="sr-only">Up and Down move, Enter chooses, Back closes.</SheetDescription>
          </SheetHeader>
        )}
        <div ref={body} className="min-h-0 flex-1 overflow-y-auto px-4 pb-6">
          {layer.kind === "menu" && <Menu {...props} focus={layer.focus} />}
          {layer.kind === "toc" && <Contents {...props} />}
          {layer.kind === "goto" && <GoTo {...props} control={goTo} />}
          {layer.kind === "settings" && <Settings {...props} control={settings} />}
          {layer.kind === "keys" && <Keys {...props} />}
        </div>
      </SheetContent>
    </Sheet>
  )
}

/** Focuses the row `data-reader-entry` names, or the first, once the panel shows. */
function useInitialFocus<T extends HTMLElement = HTMLDivElement>(entry: string | null) {
  const list = useRef<T>(null)
  useEffect(() => {
    let frame = 0
    // A Radix slider hides its thumb until its second render, and a hidden
    // thumb refuses focus, so the focus is tried again for a few frames.
    const attempt = (left: number) => {
      const rows = [...(list.current?.querySelectorAll<HTMLElement>(ROW) ?? [])]
      focusRow(rows.find((row) => row.dataset.readerEntry === entry) ?? rows[0])
      if (left > 0 && !list.current?.contains(document.activeElement)) frame = requestAnimationFrame(() => attempt(left - 1))
    }
    attempt(5)
    return () => cancelAnimationFrame(frame)
  }, [entry])
  return list
}

function Row({ entry, children, className, onSelect }: { entry: string; children: ReactNode; className?: string; onSelect(): void }) {
  return (
    <Button
      variant="ghost"
      data-reader-row
      data-reader-entry={entry}
      className={cn("h-auto min-h-11 w-full justify-start text-left whitespace-normal", className)}
      onClick={onSelect}
    >
      {children}
    </Button>
  )
}

function Menu({ focus, run, position, layout, leave }: PanelsProps & { focus: MenuEntry }) {
  const list = useInitialFocus<HTMLElement>(focus)
  const open = (panel: Exclude<MenuEntry, "close">) => run({ kind: "open", layer: panel })
  const back = (command: ReaderCommand) => {
    run({ kind: "back" })
    run(command)
  }
  return (
    <nav ref={list} className="flex flex-col gap-1" aria-label="Reader menu">
      <Row entry="toc" onSelect={() => open("toc")}>{layout === "fixed" ? "Contents and pages" : "Contents"}</Row>
      <Row entry="goto" onSelect={() => open("goto")}>Go to a position</Row>
      <Row entry="settings" onSelect={() => open("settings")}>Appearance</Row>
      <Row entry="previous" onSelect={() => back({ kind: "section", to: "prev" })}>Previous chapter</Row>
      <Row entry="next" onSelect={() => back({ kind: "section", to: "next" })}>Next chapter</Row>
      {position?.canReturn && (
        <Row entry="return" onSelect={() => back({ kind: "return" })}>Back to where you jumped from</Row>
      )}
      <Row entry="keys" onSelect={() => open("keys")}>Keys</Row>
      <Row entry="close" onSelect={leave}>Close the book</Row>
    </nav>
  )
}

function Contents({ toc, position, jump }: PanelsProps) {
  const current = position?.tocTarget ?? null
  const list = useInitialFocus(current === null ? null : String(current))
  if (!toc.length) return <p className="py-4 text-sm text-muted-foreground">This book has no contents.</p>
  return (
    <div ref={list} className="flex flex-col gap-0.5">
      {toc.map((entry, index) => (
        <Row
          key={`${index}:${entry.target}`}
          entry={String(entry.target)}
          className={cn(String(entry.target) === String(current) && "text-primary")}
          onSelect={() => jump({ kind: "toc", target: entry.target })}
        >
          <span style={{ paddingInlineStart: `${entry.depth * 1.25}rem` }}>{entry.label || `Entry ${index + 1}`}</span>
        </Row>
      ))}
    </div>
  )
}

function GoTo({ layout, position, jump, control }: Omit<PanelsProps, "control"> & { control: Ref<{ confirm(): void } | null> }) {
  const pages = position?.page?.total ?? null
  const fixed = layout === "fixed" && pages !== null
  const [value, setValue] = useState(() => (fixed ? (position?.page?.current ?? 0) + 1 : Math.round((position?.fraction ?? 0) * 1000) / 10))
  const list = useInitialFocus("position")
  const label = fixed ? `Page ${value} of ${pages}` : `${value.toFixed(1)}%`
  const go = () => jump(fixed ? { kind: "page", page: value - 1 } : { kind: "fraction", fraction: value / 100 })
  useImperativeHandle(control, () => ({ confirm: go }))
  return (
    <div ref={list} className="flex flex-col gap-6 py-4">
      <div data-reader-row data-reader-entry="position" className="flex flex-col gap-4">
        <p className="text-3xl font-semibold tabular-nums" aria-live="polite">{label}</p>
        <Slider
          aria-label="Position in the book"
          aria-valuetext={label}
          min={fixed ? 1 : 0}
          max={fixed ? pages : 100}
          step={fixed ? 1 : 0.5}
          value={[value]}
          onValueChange={([next]) => setValue(next ?? value)}
        />
      </div>
      <p className="text-sm text-muted-foreground">Left and Right move, Page Up and Page Down move further, Enter goes there.</p>
      <Button onClick={go}>Go there</Button>
    </div>
  )
}

type Setting =
  | { kind: "slider"; label: string; text: string; value: number; min: number; max: number; step: number; change(value: number): void }
  | { kind: "choice"; label: string; value: string; options: readonly (readonly [string, string])[]; change(value: string): void }

/** The rows Appearance offers for this book, each with how to show and change it. */
function settingRows(layout: "reflowable" | "fixed", appearance: ReaderAppearance, change: (next: ReaderAppearance) => void): Setting[] {
  const choice = <K extends "theme" | "fit" | "lineSpacing" | "margins" | "flow">(
    key: K,
    label: string,
    options: readonly (readonly [ReaderAppearance[K], string])[],
  ): Setting => ({
    kind: "choice",
    label,
    value: appearance[key],
    options,
    change: (value) => {
      const option = options.find(([candidate]) => candidate === value)
      if (option) change({ ...appearance, [key]: option[0] })
    },
  })
  const theme = choice("theme", "Theme", [["app", "App"], ["light", "Light"], ["sepia", "Sepia"], ["dark", "Dark"]])
  if (layout === "fixed") return [choice("fit", "Fit", [["page", "Whole page"], ["width", "Page width"]]), theme]
  return [
    {
      kind: "slider",
      label: "Text size",
      text: `${appearance.fontSize}%`,
      value: appearance.fontSize,
      min: FONT_SIZE.min,
      max: FONT_SIZE.max,
      step: FONT_SIZE.step,
      change: (fontSize) => change({ ...appearance, fontSize }),
    },
    theme,
    choice("lineSpacing", "Line spacing", [["compact", "Compact"], ["normal", "Normal"], ["relaxed", "Relaxed"]]),
    choice("margins", "Margins", [["narrow", "Narrow"], ["normal", "Normal"], ["wide", "Wide"]]),
    choice("flow", "Layout", [["paginated", "Pages"], ["scrolled", "Scrolling"]]),
  ]
}

/**
 * Appearance applies each change at once. The reader's keys own Up, Down,
 * Left and Right here, so a slider or radio group never takes Up and Down,
 * and Left and Right change exactly the focused row.
 */
function Settings({ layout, appearance, changeAppearance, control }: Omit<PanelsProps, "control"> & { control: Ref<{ adjust(by: -1 | 1): void } | null> }) {
  const list = useInitialFocus(null)
  const rows = settingRows(layout, appearance, changeAppearance)
  useImperativeHandle(control, () => ({
    adjust(by) {
      const elements = [...(list.current?.querySelectorAll<HTMLElement>(ROW) ?? [])]
      const index = elements.findIndex((element) => element.contains(document.activeElement))
      const row = rows[index]
      if (!row) return
      if (row.kind === "slider") row.change(Math.min(row.max, Math.max(row.min, row.value + by * row.step)))
      else {
        const at = row.options.findIndex(([option]) => option === row.value)
        const next = row.options[(at + by + row.options.length) % row.options.length]
        if (next) row.change(next[0])
      }
      requestAnimationFrame(() => focusRow(elements[index]))
    },
  }))
  return (
    <div ref={list} className="flex flex-col gap-4 py-2">
      {rows.map((row) => (
        <div key={row.label} data-reader-row className="flex flex-col gap-3 rounded-md p-2 focus-within:bg-accent">
          <p className="text-sm font-medium">{row.kind === "slider" ? `${row.label}: ${row.text}` : row.label}</p>
          {row.kind === "slider" ? (
            <Slider
              aria-label={row.label}
              aria-valuetext={row.text}
              min={row.min}
              max={row.max}
              step={row.step}
              value={[row.value]}
              onValueChange={([next]) => next !== undefined && row.change(next)}
            />
          ) : (
            <RadioGroup aria-label={row.label} orientation="horizontal" className="flex flex-wrap gap-x-5 gap-y-2" value={row.value} onValueChange={row.change}>
              {row.options.map(([option, text]) => (
                <Label key={option} className="flex items-center gap-2 font-normal">
                  <RadioGroupItem value={option} />
                  {text}
                </Label>
              ))}
            </RadioGroup>
          )}
        </div>
      ))}
    </div>
  )
}

const KEY_NAMES: Readonly<Record<string, string>> = {
  ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓",
  PageUp: "Page Up", PageDown: "Page Down", Escape: "Esc", Backspace: "Backspace",
  BrowserBack: "Remote Back", ContextMenu: "Menu key", Enter: "Enter / OK", Space: "Space",
  "Shift+Space": "Shift+Space", "Alt+ArrowLeft": "Alt+←",
  MediaTrackNext: "Next track", MediaTrackPrevious: "Previous track", MediaFastForward: "Fast forward", MediaRewind: "Rewind",
}

function Keys({ keyContext }: PanelsProps) {
  const list = useInitialFocus(null)
  const sections = [
    ["While reading", bindingsFor("reading", keyContext)],
    ["In the menu", bindingsFor("menu", keyContext)],
    ["In Contents", bindingsFor("toc", keyContext)],
    ["In Go to", bindingsFor("goto", keyContext)],
    ["In Appearance", bindingsFor("settings", keyContext)],
  ] as const
  return (
    <div ref={list} className="flex flex-col gap-5 py-2">
      {sections.map(([title, bindings]) => (
        <section key={title} className="flex flex-col gap-1">
          <h3 className="text-sm font-semibold">{title}</h3>
          {bindings.map((binding) => (
            <div key={binding.label + binding.chords.join()} data-reader-row tabIndex={-1} className="flex justify-between gap-4 rounded-sm px-2 py-1 text-sm outline-none focus-visible:bg-accent">
              <span>{binding.label}</span>
              <span className="text-right text-muted-foreground">{binding.chords.map((chord) => KEY_NAMES[chord] ?? chord).join(", ")}</span>
            </div>
          ))}
        </section>
      ))}
    </div>
  )
}
