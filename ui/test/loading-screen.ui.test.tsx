import { act, render } from "@testing-library/react"
import { afterEach, expect, test, vi } from "vitest"
import { LoadingScreen } from "../src/components/LoadingScreen"
import { api } from "../src/lib/api"
import { markWindowRevealed, resetWindowRevealForTests } from "../src/lib/startup"

vi.mock("../src/lib/api", () => ({ api: { shell: { windowReady: vi.fn().mockResolvedValue(undefined) } } }))

afterEach(() => {
  resetWindowRevealForTests()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

test("reveals the still-hidden startup window without waiting for a frame", async () => {
  vi.useFakeTimers()
  vi.stubGlobal("requestAnimationFrame", vi.fn())
  const view = render(<LoadingScreen ready={false} />)
  expect(api.shell.windowReady).not.toHaveBeenCalled()
  view.rerender(<LoadingScreen ready />)
  await act(() => vi.advanceTimersByTimeAsync(0))
  expect(view.queryByRole("status")).toBeNull()
  expect(api.shell.windowReady).toHaveBeenCalledTimes(1)
})

test("a revealed window whose frames stall still drops the cover after the fallback", async () => {
  vi.useFakeTimers()
  vi.stubGlobal("requestAnimationFrame", vi.fn())
  markWindowRevealed()
  const view = render(<LoadingScreen ready />)
  await act(() => vi.advanceTimersByTimeAsync(1_000))
  expect(view.queryByRole("status")).toBeNull()
  expect(api.shell.windowReady).toHaveBeenCalledTimes(1)
})

test("cancelled readiness cannot reveal the window", async () => {
  vi.useFakeTimers()
  vi.stubGlobal("requestAnimationFrame", vi.fn())
  const view = render(<LoadingScreen ready />)
  view.rerender(<LoadingScreen ready={false} />)
  await act(() => vi.advanceTimersByTimeAsync(200))
  expect(api.shell.windowReady).not.toHaveBeenCalled()
  expect(view.queryByRole("status")).not.toBeNull()
})
