import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { createMemoryRouter, RouterProvider } from "react-router-dom"
import { afterEach, describe, expect, test, vi } from "vitest"
import { RootErrorScreen } from "@/components/RootErrorScreen"
import { api } from "@/lib/api"
import { formatRootErrorSummary } from "@/lib/root-error"
import { resetWindowRevealed, signalWindowReady } from "@/lib/startup"
import { createAppRoutes } from "@/router"

function CrashDuringRender(): never {
  throw new Error("boom while rendering with https://jf.local/Items?api_key=supersecret123&token=tok999")
}

afterEach(() => {
  vi.restoreAllMocks()
  resetWindowRevealed()
})

describe("root route error recovery", () => {
  test("default root route catches an unhandled render error and signals windowReady", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})
    const ready = vi.spyOn(api.shell, "windowReady").mockResolvedValue({ queued: true })
    const router = createMemoryRouter(createAppRoutes(<CrashDuringRender />), {
      initialEntries: ["/"],
    })

    render(<RouterProvider router={router} />)

    expect(await screen.findByRole("heading", { name: "Something went wrong" })).toBeTruthy()
    await waitFor(() => expect(ready).toHaveBeenCalledTimes(1))
    consoleError.mockRestore()
  })

  test("skips signalling windowReady when the window was already revealed", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})
    const ready = vi.spyOn(api.shell, "windowReady").mockResolvedValue({ queued: true })
    signalWindowReady()
    ready.mockClear()

    const router = createMemoryRouter(createAppRoutes(<CrashDuringRender />), {
      initialEntries: ["/"],
    })

    render(<RouterProvider router={router} />)

    expect(await screen.findByRole("heading", { name: "Something went wrong" })).toBeTruthy()
    expect(ready).not.toHaveBeenCalled()
    consoleError.mockRestore()
  })

  test("redacts secrets in the error summary and invokes recovery reload", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})
    const ready = vi.spyOn(api.shell, "windowReady").mockRejectedValue(new Error("api down"))
    const onReload = vi.fn()
    const router = createMemoryRouter(createAppRoutes(<CrashDuringRender />, <RootErrorScreen onReload={onReload} />), {
      initialEntries: ["/library"],
    })

    render(<RouterProvider router={router} />)

    expect(await screen.findByRole("heading", { name: "Something went wrong" })).toBeTruthy()
    await waitFor(() => expect(ready).toHaveBeenCalledTimes(1))

    const summary = screen.getByText(/boom while rendering/i)
    expect(summary.textContent).not.toContain("supersecret123")
    expect(summary.textContent).not.toContain("tok999")

    fireEvent.click(screen.getByRole("button", { name: /reload/i }))
    expect(onReload).toHaveBeenCalledTimes(1)
    consoleError.mockRestore()
  })

  test("redacts sensitive credentials and handles non-Error values in error summaries", () => {
    const withSecrets = new Error(
      "Request failed: Authorization: Bearer eyJhbGciOiJIUzI1NiJ9 X-Emby-Token=secret-token password=hunter2",
    )
    const redacted = formatRootErrorSummary(withSecrets)
    expect(redacted).not.toContain("eyJhbGciOiJIUzI1NiJ9")
    expect(redacted).not.toContain("secret-token")
    expect(redacted).not.toContain("hunter2")

    expect(formatRootErrorSummary({ status: 500, statusText: "Internal Error", data: null })).not.toBe("")
    expect(formatRootErrorSummary(null)).not.toBe("")
  })
})
