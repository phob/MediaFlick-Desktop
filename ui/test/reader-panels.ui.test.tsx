import { render, screen, waitFor } from "@testing-library/react"
import { createRef } from "react"
import { afterEach, describe, expect, test, vi } from "vitest"
import type { ReaderAppearance } from "@/lib/api"
import type { ReaderLayer } from "@/reader/model"
import { ReaderPanels, type PanelControl } from "@/reader/panels"

const DEFAULT_APPEARANCE: ReaderAppearance = {
  theme: "app",
  fontSize: 100,
  lineSpacing: "normal",
  margins: "normal",
  flow: "paginated",
  fit: "page",
}

function renderPanel(layer: ReaderLayer, layout: "reflowable" | "fixed" = "reflowable") {
  const control = createRef<PanelControl | null>()
  return render(
    <ReaderPanels
      layer={layer}
      control={control}
      layout={layout}
      toc={[{ label: "Chapter 1", target: "ch1", depth: 0 }]}
      position={{
        fraction: 0.25,
        finished: false,
        chapter: "Chapter 1",
        tocTarget: "ch1",
        page: layout === "fixed" ? { current: 1, total: 10 } : null,
        textLocation: null,
        location: layout === "fixed"
          ? { kind: "page", page: 1, pages: 10 }
          : { kind: "text", cfi: "epubcfi(/6/2!/4/2)", fraction: 0.25 },
        canReturn: false,
      }}
      appearance={DEFAULT_APPEARANCE}
      keyContext={{ flow: "paginated", reflowable: layout === "reflowable" }}
      run={() => {}}
      jump={() => {}}
      changeAppearance={() => {}}
      leave={() => {}}
    />,
  )
}

function guardHiddenFocus() {
  const originalFocus = HTMLElement.prototype.focus
  return vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (this: HTMLElement, options) {
    if (this.style.display === "none") return
    originalFocus.call(this, options)
  })
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe("reader panel initial focus", () => {
  test("focuses the Go to slider thumb deterministically when Radix unhides it without rAF polling", async () => {
    guardHiddenFocus()
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation(() => 1)

    renderPanel({ kind: "goto", from: "menu" }, "reflowable")
    const slider = screen.getByRole("slider", { name: "Position in the book" })

    await waitFor(() => {
      expect(document.activeElement).toBe(slider)
    })
    expect(raf).not.toHaveBeenCalled()
  })

  test("focuses the reflowable Appearance slider thumb deterministically without rAF polling", async () => {
    guardHiddenFocus()
    const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation(() => 1)

    renderPanel({ kind: "settings", from: "menu" }, "reflowable")
    const slider = screen.getByRole("slider", { name: "Text size" })

    await waitFor(() => {
      expect(document.activeElement).toBe(slider)
    })
    expect(raf).not.toHaveBeenCalled()
  })

  test("focuses the requested menu entry and checked radio immediately", () => {
    const raf = vi.spyOn(window, "requestAnimationFrame")

    const { unmount } = renderPanel({ kind: "menu", focus: "settings" })
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Appearance" }))
    unmount()

    renderPanel({ kind: "settings", from: "menu" }, "fixed")
    expect(document.activeElement).toBe(screen.getByRole("radio", { name: "Whole page" }))
    expect(raf).not.toHaveBeenCalled()
  })
})
