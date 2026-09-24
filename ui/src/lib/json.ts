export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[]

export interface JsonObject {
  [key: string]: JsonValue
}

/** Decode a JSON object without accepting arrays, boxed primitives, or class instances. */
export function isJsonObject(value: JsonValue | undefined): value is JsonObject {
  return value !== undefined
    && value !== null
    && !Array.isArray(value)
    && Object.getPrototypeOf(value) === Object.prototype
}

export function jsonString(value: JsonValue | undefined): string | null {
  return Object.prototype.toString.call(value) === "[object String]" ? String(value) : null
}

export function jsonNumber(value: JsonValue | undefined): number | null {
  if (Object.prototype.toString.call(value) !== "[object Number]") return null
  const number = Number(value)
  return Number.isFinite(number) ? number : null
}

export function jsonBoolean(value: JsonValue | undefined): boolean | null {
  if (value === true) return true
  if (value === false) return false
  return null
}

export function jsonStringArray(value: JsonValue | undefined): string[] | null {
  if (!Array.isArray(value)) return null
  const result: string[] = []
  for (const entry of value) {
    const decoded = jsonString(entry)
    if (decoded === null) return null
    result.push(decoded)
  }
  return result
}

/**
 * Compares values as JSON would serialize them, ignoring object key order.
 * The shell may send the same settings with keys in different orders, for
 * example when one response passes through `serde_json::Value` and another
 * serializes a struct directly.
 */
export function sameJson(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true
  if (typeof left !== "object" || typeof right !== "object" || left === null || right === null) return false
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left) && Array.isArray(right) && left.length === right.length
      && left.every((entry, index) => sameJson(entry, right[index]))
  }
  const leftEntries = Object.entries(left).filter(([, value]) => value !== undefined)
  const rightObject = right as Record<string, unknown>
  const rightSize = Object.values(rightObject).filter((value) => value !== undefined).length
  return leftEntries.length === rightSize
    && leftEntries.every(([key, value]) => sameJson(value, rightObject[key]))
}
