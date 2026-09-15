import { useState } from "react"
import { Keyboard, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { shortcutFromEvent, shortcutLabel } from "@/lib/player-shortcuts"

export function ShortcutRecorder({ id, label, value, onChange, disabled = false }: {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
  disabled?: boolean
}) {
  const [recording, setRecording] = useState(false)
  const [message, setMessage] = useState("")
  // Keep capture prompts and status messages from resizing the settings column.
  return <div className="w-64 max-w-full min-w-0 space-y-2" data-shortcut-recorder>
    <div className="flex items-center gap-2">
      <Button id={id} type="button" variant="outline" className="min-w-0 flex-1 shrink justify-start aria-pressed:border-primary aria-pressed:bg-primary/10" aria-label={label} aria-describedby={`${id}-capture-help`} aria-pressed={recording} disabled={disabled} title={value ? shortcutLabel(value) : "Disabled"}
        onClick={() => { setRecording(true); setMessage("Press a key combination. Escape cancels; Tab moves on.") }}
        onBlur={() => { setRecording(false); setMessage("") }}
        onKeyDown={(event) => {
          if (!recording) return
          event.stopPropagation()
          if (event.key === "Tab") { setRecording(false); setMessage(""); return }
          event.preventDefault()
          if (event.key === "Escape") { setRecording(false); setMessage("Recording cancelled."); return }
          if (event.repeat || event.nativeEvent.isComposing) return
          if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return
          const binding = shortcutFromEvent(event)
          if (!binding) { setMessage("Try a letter, number, function key, or navigation key."); return }
          if (binding === "Alt+F4") { setMessage("Alt+F4 is reserved for closing the window."); return }
          onChange(binding)
          setRecording(false)
          setMessage("Recorded. Save to apply.")
        }}>
        <Keyboard className="size-4 text-muted-foreground" />
        {recording ? <span className="truncate">Press a combination…</span> : value ? <kbd className="truncate font-mono text-sm">{shortcutLabel(value)}</kbd> : "Disabled"}
      </Button>
      <Button type="button" variant="ghost" size="icon" aria-label={`Clear ${label.toLowerCase()}`} disabled={disabled || !value} onClick={() => { setRecording(false); setMessage(""); onChange("") }}><X /></Button>
    </div>
    <p id={`${id}-capture-help`} role="status" className="min-h-[2lh] text-xs text-muted-foreground">{message || "Click or press Enter to record a shortcut."}</p>
  </div>
}
