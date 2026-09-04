import { act, render } from "@testing-library/react"
import { afterEach, expect, test, vi } from "vitest"
import { LoadingScreen } from "../src/components/LoadingScreen"
import { api } from "../src/lib/api"

vi.mock("../src/lib/api", () => ({ api: { shell: { windowReady: vi.fn().mockResolvedValue(undefined) } } }))

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

test("reveals the usable route with stalled artwork and suspended hidden-window paints", async () => {
  vi.useFakeTimers()
  vi.stubGlobal("requestAnimationFrame", vi.fn())
  const view = render(<><img src="pending.jpg" alt="Pending artwork" /><LoadingScreen ready={false} /></>)
  expect(api.shell.windowReady).not.toHaveBeenCalled()
  view.rerender(<><img src="pending.jpg" alt="Pending artwork" /><LoadingScreen ready /></>)
  await act(() => vi.advanceTimersByTimeAsync(100))
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
