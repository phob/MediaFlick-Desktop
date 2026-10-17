import type * as Wire from "./api/wire.gen.ts"

export type ShellEvent = Wire.ShellEvent

export function readShellEvent(event: Event): ShellEvent | null {
  if (!(event instanceof CustomEvent)) return null
  // Only the native shell dispatches this event, and it sends a Rust
  // `ShellEvent` (src/shell/cef/ui_events.rs), whose generated type this is.
  const shellEvent: ShellEvent = event.detail
  return shellEvent
}
