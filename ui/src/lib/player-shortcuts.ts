import type { PlayerComfort } from "./api"

export const PLAYER_SHORTCUTS = [
  ["pauseKey", "Pause key"], ["muteKey", "Mute key"], ["fullscreenKey", "Fullscreen key"],
  ["seekBackKey", "Seek backward key"], ["seekForwardKey", "Seek forward key"],
  ["stopKey", "Stop playback key"], ["subtitlesKey", "Toggle subtitles key"],
  ["seekBackThirtyKey", "Seek backward 30 seconds key"],
  ["seekForwardThirtyKey", "Seek forward 30 seconds key"],
] as const satisfies ReadonlyArray<readonly [keyof PlayerComfort, string]>

const NAMED_KEYS: Record<string, string> = {
  " ": "SPACE", Enter: "ENTER", Tab: "TAB", Escape: "ESC", Backspace: "BS", Delete: "DEL",
  Insert: "INS", Home: "HOME", End: "END", PageUp: "PGUP", PageDown: "PGDWN",
  ArrowUp: "UP", ArrowDown: "DOWN", ArrowLeft: "LEFT", ArrowRight: "RIGHT",
}
const RESERVED_KEYS = new Set(["SPACE", "LEFT", "RIGHT", "ESC", "TAB", "Alt+F4"])

export function normalizeShortcut(binding: string): string | null {
  if (!binding.trim()) return ""
  if (binding.length > 80) return null
  const parts = binding.trim().split("+")
  let key = parts.pop() ?? ""
  const modifiers = new Set<string>()
  for (const part of parts) {
    const name = ({ctrl:"Ctrl", control:"Ctrl", alt:"Alt", shift:"Shift", meta:"Meta", super:"Meta"} as Record<string, string>)[part.toLowerCase()]
    if (!name) return null
    modifiers.add(name)
  }
  if (/^[a-z0-9]$/i.test(key)) {
    if (/^[A-Z]$/.test(key)) modifiers.add("Shift")
    key = key.toLowerCase()
  } else {
    key = key.toUpperCase()
    if (!Object.values(NAMED_KEYS).includes(key) && !/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return null
  }
  return [...["Ctrl", "Alt", "Shift", "Meta"].filter((name) => modifiers.has(name)), key].join("+")
}

export function shortcutFromEvent(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey"> & {code?: string}): string | null {
  const physicalKey = (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) && /^(Key[A-Z]|Digit[0-9])$/.test(event.code ?? "") ? event.code?.replace(/^(Key|Digit)/, "").toLowerCase() : null
  const key = /^[a-z0-9]$/i.test(event.key) ? event.key.toLowerCase() : NAMED_KEYS[event.key] ?? (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.key) ? event.key : physicalKey)
  if (!key) return null
  return [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Meta", key].filter(Boolean).join("+")
}

export function shortcutError(comfort: PlayerComfort, watchedNext: string | null): string | null {
  const keys = PLAYER_SHORTCUTS.map(([key]) => normalizeShortcut(comfort[key]))
  if (keys.some((key) => key === null)) return "Record a supported key combination."
  if (keys.some((key) => key && (RESERVED_KEYS.has(key) || key.split("+").at(-1) === "F11"))) return "Space and Left/Right arrows are always available. Escape, Tab, F11, and Alt+F4 are reserved for navigation."
  const enabled = keys.filter(Boolean)
  if (new Set(enabled).size !== enabled.length) return "Assign each key combination to only one action."
  const watched = normalizeShortcut(watchedNext ?? "")
  if (watched === null) return "Record a supported mark watched key in Settings → Client → Player."
  if (watched && (enabled.includes(watched) || (RESERVED_KEYS.has(watched) || watched.split("+").at(-1) === "F11"))) {
    return "This conflicts with the mark watched key. Assign each key combination to only one action."
  }
  return null
}

export function shortcutLabel(binding: string): string {
  const meta = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform) ? "⌘" : "Meta"
  return (normalizeShortcut(binding) ?? binding).split("+").map((part) => part.length === 1 ? part.toUpperCase() : ({Meta:meta, SPACE:"Space", UP:"↑", DOWN:"↓", LEFT:"←", RIGHT:"→", ENTER:"Enter", ESC:"Esc", TAB:"Tab", BS:"Backspace", DEL:"Delete", INS:"Insert", HOME:"Home", END:"End", PGUP:"Page Up", PGDWN:"Page Down"} as Record<string, string>)[part] ?? part).join(" + ")
}
