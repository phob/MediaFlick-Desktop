import { isJsonObject, jsonString, jsonStringArray, type JsonObject, type JsonValue } from "./json.ts"

export interface ShellEvent {
  type: string
  payload: JsonObject
}

export function readShellEvent(event: Event): ShellEvent | null {
  if (!(event instanceof CustomEvent)) return null
  const detail: JsonValue = event.detail
  if (!isJsonObject(detail)) return null
  const type = jsonString(detail.type)
  if (!type) return null
  return {
    type,
    payload: isJsonObject(detail.payload) ? detail.payload : {},
  }
}

export function shellEventIds(value: JsonValue | undefined): string[] {
  return jsonStringArray(value) ?? []
}
