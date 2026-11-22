import type * as Wire from "./api/wire.gen.ts"

export type ShellEvent = Wire.ShellEvent

export function readShellEvent(event: Event): ShellEvent | null {
  if (!(event instanceof CustomEvent)) return null
  const shellEvent: ShellEvent = event.detail
  return shellEvent
}
