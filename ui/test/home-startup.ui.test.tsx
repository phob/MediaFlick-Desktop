import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test, vi } from "vitest"
import type { ReactNode } from "react"

const queryState = vi.hoisted(() => ({
  homePending: true,
  billboardPending: true,
}))

vi.mock("@/lib/queries", () => ({
  useStatus: () => ({
    data: { authenticated: true, libraryReady: true },
    isPending: false,
  }),
  useHome: () => ({ isPending: queryState.homePending }),
  useBillboard: () => ({ isPending: queryState.billboardPending }),
}))
vi.mock("@/components/AppShell", () => ({
  AppShell: ({ children }: { children: ReactNode }) => <div data-testid="app-shell">{children}</div>,
}))
vi.mock("@/components/LoadingScreen", () => ({
  LoadingScreen: ({ ready }: { ready: boolean }) => (
    <div data-testid="loading-screen" data-ready={String(ready)} />
  ),
}))
vi.mock("@/components/seerr/SeerrGate", () => ({
  SeerrGate: ({ children }: { children: ReactNode }) => children,
}))
vi.mock("@/lib/ratings", () => ({
  RatingsProvider: ({ children }: { children: ReactNode }) => <div data-testid="ratings-provider">{children}</div>,
}))
vi.mock("@/lib/technical", () => ({
  TechnicalProvider: ({ children }: { children: ReactNode }) => children,
}))
vi.mock("@/routes/Home", () => ({ default: () => <div>Home content</div> }))
vi.mock("@/routes/Library", () => ({ default: () => null }))
vi.mock("@/routes/ItemDetail", () => ({ default: () => null }))
vi.mock("@/routes/Calendar", () => ({ default: () => null }))
vi.mock("@/routes/Discover", () => ({ default: () => null }))
vi.mock("@/routes/DiscoverDetail", () => ({ default: () => null }))
vi.mock("@/routes/Requests", () => ({ default: () => null }))
vi.mock("@/routes/SignIn", () => ({ default: () => null }))
vi.mock("@/routes/Settings", () => ({
  default: () => null,
  AppearanceSync: () => null,
}))

import App from "@/App"

describe("home startup cover", () => {
  test("keeps the complete shell, including its preview portal, under rating context", () => {
    const view = render(
      <MemoryRouter>
        <App />
      </MemoryRouter>,
    )

    const ratings = screen.getByTestId("ratings-provider")
    const shell = screen.getByTestId("app-shell")
    expect(ratings.contains(shell)).toBe(true)
    view.unmount()
  })

  test("stays up until both SQLite-backed home queries settle", () => {
    const view = render(
      <MemoryRouter>
        <App />
      </MemoryRouter>,
    )

    expect(screen.getByTestId("loading-screen").getAttribute("data-ready")).toBe("false")

    queryState.homePending = false
    view.rerender(
      <MemoryRouter>
        <App />
      </MemoryRouter>,
    )
    expect(screen.getByTestId("loading-screen").getAttribute("data-ready")).toBe("false")

    queryState.billboardPending = false
    view.rerender(
      <MemoryRouter>
        <App />
      </MemoryRouter>,
    )
    expect(screen.getByTestId("loading-screen").getAttribute("data-ready")).toBe("true")
  })
})
