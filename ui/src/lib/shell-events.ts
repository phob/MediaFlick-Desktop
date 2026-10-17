import type * as Wire from "./api/wire.gen.ts"
import { isJsonObject, jsonString, type JsonValue } from "./json.ts"

export type ShellEvent = Wire.ShellEvent

export function readShellEvent(event: Event): ShellEvent | null {
  if (!(event instanceof CustomEvent)) return null
  const detail: JsonValue = event.detail
  if (!isJsonObject(detail) || jsonString(detail.type) === null) return null
  // Only the native shell dispatches this event, and it sends a Rust
  // `ShellEvent` (src/shell/cef/ui_events.rs), whose generated type this is.
  const shellEvent: ShellEvent = event.detail
  return shellEvent
}
