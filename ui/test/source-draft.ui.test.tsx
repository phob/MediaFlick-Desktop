import { act, renderHook } from "@testing-library/react"
import { expect, test } from "vitest"
import { useSourceDraft } from "../src/hooks/use-source-draft"

test("refreshes merge untouched fields and preserve edits until discard", () => {
  const original = { name: "Original", enabled: false }
  const { result, rerender } = renderHook(({ source }) => useSourceDraft(source), { initialProps: { source: original } })
  act(() => result.current[1]({ ...original, name: "Edited" }))
  const refreshed = { name: "Server", enabled: true }
  rerender({ source: refreshed })
  expect(result.current[0]).toEqual({ name: "Edited", enabled: true })
  act(() => result.current[1](refreshed))
  expect(result.current[0]).toEqual(refreshed)
})

test("a normalized save keeps changes made after submission", () => {
  const original = { name: "Original", enabled: false }
  const submitted = { name: "  Edited  ", enabled: false }
  const { result, rerender } = renderHook(({ source }) => useSourceDraft(source), { initialProps: { source: original } })
  act(() => result.current[1](submitted))
  act(() => result.current[2]((current) => ({ ...current, enabled: true })))
  const saved = { name: "Edited", enabled: false }
  act(() => result.current[3](saved, submitted))
  rerender({ source: saved })
  expect(result.current[0]).toEqual({ name: "Edited", enabled: true })
})

test("account changes discard the previous account's draft", () => {
  const source = { name: "Original" }
  const { result, rerender } = renderHook(({ account }) => useSourceDraft(source, account), { initialProps: { account: "one" } })
  act(() => result.current[1]({ name: "Edited" }))
  rerender({ account: "two" })
  expect(result.current[0]).toEqual(source)
})
