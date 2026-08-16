import { fireEvent, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom"
import { beforeEach, describe, expect, test, vi } from "vitest"

vi.mock("@/components/AppSidebar", () => ({ AppSidebar: () => null }))
vi.mock("@/components/PlayerBar", () => ({ PlayerBar: () => null }))
vi.mock("@/components/PreviewCard", () => ({ PreviewProvider: ({ children }: { children: ReactNode }) => children }))
vi.mock("@/components/ui/sidebar", () => ({
  SidebarInset: ({ children, className }: { children: ReactNode; className?: string }) => <main className={className}>{children}</main>,
  SidebarProvider: ({ children, className }: { children: ReactNode; className?: string }) => <div className={className}>{children}</div>,
}))
vi.mock("@/lib/playback-events", () => ({ usePlaybackStoppedBridge: () => undefined }))
vi.mock("@/lib/library-events", () => ({ useLibraryMetadataBridge: () => undefined }))

import { AppShell } from "@/components/AppShell"

function First() {
  const navigate = useNavigate()
  return <button onClick={() => navigate("/second")}>Second</button>
}

function Second() {
  const navigate = useNavigate()
  return <button onClick={() => navigate(-1)}>Back</button>
}

describe("AppShell route scrolling", () => {
  beforeEach(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value({ top }: ScrollToOptions) {
        this.scrollTop = top ?? 0
      },
    })
  })

  test("new routes start at the top and browser Back restores the old route", () => {
    const view = render(
      <MemoryRouter initialEntries={["/first"]}>
        <AppShell>
          <Routes>
            <Route path="/first" element={<First />} />
            <Route path="/second" element={<Second />} />
          </Routes>
        </AppShell>
      </MemoryRouter>,
    )
    const viewport = view.container.querySelector<HTMLElement>(".content-viewport")!
    viewport.scrollTop = 180

    fireEvent.click(screen.getByRole("button", { name: "Second" }))
    expect(viewport.scrollTop).toBe(0)

    viewport.scrollTop = 90
    fireEvent.click(screen.getByRole("button", { name: "Back" }))
    expect(viewport.scrollTop).toBe(180)
  })
})
