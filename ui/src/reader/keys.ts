import type { LayerKind, ReaderCommand } from "./model"

/**
 * A key as the table writes it: `KeyboardEvent.key` (a space is `Space`),
 * after the modifiers held, as in `Shift+Space` or `Alt+ArrowLeft`, or a
 * mouse button on the book.
 */
type Chord = string

export type MouseChord = "Click" | "RightClick"

interface Binding {
  chords: readonly Chord[]
  command: ReaderCommand
  label: string
  repeat?: true
  flow?: "paginated" | "scrolled"
  reflowable?: true
}

const BACK: Binding = {
  chords: ["Escape", "Backspace", "BrowserBack", "Alt+ArrowLeft"],
  command: { kind: "back" },
  label: "Close this panel",
}

const LIST: readonly Binding[] = [
  { chords: ["ArrowUp"], command: { kind: "move", to: "previous" }, label: "Previous entry", repeat: true },
  { chords: ["ArrowDown"], command: { kind: "move", to: "next" }, label: "Next entry", repeat: true },
  { chords: ["PageUp"], command: { kind: "move", to: "previousTen" }, label: "Ten entries up", repeat: true },
  { chords: ["PageDown"], command: { kind: "move", to: "nextTen" }, label: "Ten entries down", repeat: true },
  { chords: ["Home"], command: { kind: "move", to: "first" }, label: "First entry" },
  { chords: ["End"], command: { kind: "move", to: "last" }, label: "Last entry" },
]

/**
 * A key the table leaves out belongs to the focused control (a button's
 * Enter, a slider's arrows), and Play/Pause and Stop always belong to playback.
 */
const KEYMAP: Readonly<Record<LayerKind, readonly Binding[]>> = {
  reading: [
    { chords: ["ArrowRight"], command: { kind: "turn", to: "right" }, label: "Page to the right", repeat: true },
    { chords: ["ArrowLeft"], command: { kind: "turn", to: "left" }, label: "Page to the left", repeat: true },
    { chords: ["PageDown", "Space", "Click"], command: { kind: "turn", to: "next" }, label: "Next page", repeat: true },
    { chords: ["PageUp", "Shift+Space", "RightClick"], command: { kind: "turn", to: "prev" }, label: "Previous page", repeat: true },
    { chords: ["ArrowDown"], command: { kind: "turn", to: "next" }, label: "Next page", repeat: true, flow: "paginated" },
    { chords: ["ArrowUp"], command: { kind: "turn", to: "prev" }, label: "Previous page", repeat: true, flow: "paginated" },
    { chords: ["ArrowDown"], command: { kind: "scroll", by: 1 }, label: "Scroll down", repeat: true, flow: "scrolled" },
    { chords: ["ArrowUp"], command: { kind: "scroll", by: -1 }, label: "Scroll up", repeat: true, flow: "scrolled" },
    { chords: ["MediaTrackNext", "]", "MediaFastForward"], command: { kind: "section", to: "next" }, label: "Next chapter" },
    { chords: ["MediaTrackPrevious", "[", "MediaRewind"], command: { kind: "section", to: "prev" }, label: "Previous chapter" },
    { chords: ["Home"], command: { kind: "edge", to: "start" }, label: "Start of the book" },
    { chords: ["End"], command: { kind: "edge", to: "end" }, label: "End of the book" },
    { chords: ["b"], command: { kind: "return" }, label: "Back to where you jumped from" },
    { chords: ["+", "="], command: { kind: "font", step: 1 }, label: "Larger text", reflowable: true },
    { chords: ["-"], command: { kind: "font", step: -1 }, label: "Smaller text", reflowable: true },
    { chords: ["Enter", "ContextMenu", "m"], command: { kind: "open", layer: "menu" }, label: "Menu" },
    { chords: ["t"], command: { kind: "open", layer: "toc" }, label: "Contents" },
    { chords: ["g"], command: { kind: "open", layer: "goto" }, label: "Go to a position" },
    { chords: ["a"], command: { kind: "open", layer: "settings" }, label: "Appearance" },
    { chords: ["?"], command: { kind: "open", layer: "keys" }, label: "Keys" },
    { ...BACK, label: "Close the book" },
  ],
  menu: [
    { chords: ["ArrowUp"], command: { kind: "move", to: "previous" }, label: "Previous entry", repeat: true },
    { chords: ["ArrowDown"], command: { kind: "move", to: "next" }, label: "Next entry", repeat: true },
    { ...BACK, chords: [...BACK.chords, "ContextMenu"], label: "Back to the book" },
  ],
  toc: [...LIST, BACK],
  goto: [{ chords: ["Enter"], command: { kind: "confirm" }, label: "Go there" }, BACK],
  settings: [
    { chords: ["ArrowUp"], command: { kind: "move", to: "previous" }, label: "Previous setting", repeat: true },
    { chords: ["ArrowDown"], command: { kind: "move", to: "next" }, label: "Next setting", repeat: true },
    { chords: ["ArrowLeft"], command: { kind: "adjust", by: -1 }, label: "Smaller, or the previous choice", repeat: true },
    { chords: ["ArrowRight"], command: { kind: "adjust", by: 1 }, label: "Larger, or the next choice", repeat: true },
    BACK,
  ],
  keys: [...LIST, BACK],
}

export interface KeyContext {
  flow: "paginated" | "scrolled"
  reflowable: boolean
}

function chordOf(event: KeyboardEvent): Chord {
  const key = event.key === " " ? "Space" : event.key
  const shifted = event.shiftKey && key.length > 1 ? "Shift+" : ""
  return `${event.ctrlKey ? "Ctrl+" : ""}${event.altKey ? "Alt+" : ""}${event.metaKey ? "Meta+" : ""}${shifted}${key}`
}

function applies(binding: Binding, context: KeyContext) {
  return (
    (!binding.flow || binding.flow === (context.reflowable ? context.flow : "paginated")) &&
    (!binding.reflowable || context.reflowable)
  )
}

export function commandFor(layer: LayerKind, event: KeyboardEvent, context: KeyContext): ReaderCommand | null {
  if (event.isComposing) return null
  const chord = chordOf(event)
  const binding = KEYMAP[layer].find((candidate) => candidate.chords.includes(chord) && applies(candidate, context))
  if (!binding || (event.repeat && !binding.repeat)) return null
  return binding.command
}

export function commandForMouse(layer: LayerKind, chord: MouseChord, context: KeyContext): ReaderCommand | null {
  return KEYMAP[layer].find((binding) => binding.chords.includes(chord) && applies(binding, context))?.command ?? null
}

export function bindingsFor(layer: LayerKind, context: KeyContext) {
  return KEYMAP[layer].filter((binding) => applies(binding, context))
}
