export const READER_ROUTE = "/read/:id"

export type ReaderEntry = "resume" | "start"

export function readerPath(itemId: string, entry: ReaderEntry) {
  return `/read/${encodeURIComponent(itemId)}${entry === "start" ? "?from=start" : ""}`
}
