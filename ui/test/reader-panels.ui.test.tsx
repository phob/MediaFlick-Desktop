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
  test("focuses the Go to slider thumb once it becomes visible", async () => {
    guardHiddenFocus()

    const host = document.createElement("div")
    host.tabIndex = -1
    document.body.append(host)
    host.focus()

    try {
      renderPanel({ kind: "goto", from: "reading" }, "reflowable")
      const slider = screen.getByRole("slider", { name: "Position in the book" })

      await waitFor(() => {
        expect(document.activeElement).toBe(slider)
      })
    } finally {
      host.remove()
    }
  })

  test("focuses the reflowable Appearance slider thumb once it becomes visible", async () => {
    guardHiddenFocus()

    renderPanel({ kind: "settings", from: "menu" }, "reflowable")
    const slider = screen.getByRole("slider", { name: "Text size" })

    await waitFor(() => {
      expect(document.activeElement).toBe(slider)
    })
  })

  test("focuses the requested menu entry and checked radio immediately", () => {
    const { unmount } = renderPanel({ kind: "menu", focus: "settings" })
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Appearance" }))
    unmount()

    renderPanel({ kind: "settings", from: "menu" }, "fixed")
    expect(document.activeElement).toBe(screen.getByRole("radio", { name: "Whole page" }))
  })

  test("does not steal focus back if focus moves to another control before the slider thumb unhides", async () => {
    guardHiddenFocus()
    const originalObserve = MutationObserver.prototype.observe
    let movedFocus = false
    vi.spyOn(MutationObserver.prototype, "observe").mockImplementation(function (
      this: MutationObserver,
      target: Node,
      options?: MutationObserverInit,
    ) {
      originalObserve.call(this, target, options)
      if (target instanceof Element && target.closest("[data-reader-panel]") && !movedFocus) {
        movedFocus = true
        const close = document.querySelector<HTMLElement>('[data-slot="sheet-content"] > button')
        close?.focus()
      }
    })

    renderPanel({ kind: "goto", from: "menu" }, "reflowable")
    const slider = screen.getByRole("slider", { name: "Position in the book" })
    const close = screen.getByRole("button", { name: "Close" })

    await waitFor(() => {
      expect(slider.style.display).not.toBe("none")
    })
    await Promise.resolve()

    expect(movedFocus).toBe(true)
    expect(document.activeElement).toBe(close)
  })

  test("cancels pending slider focus if the user interacts via keyboard before the thumb unhides", async () => {
    guardHiddenFocus()
    const originalObserve = MutationObserver.prototype.observe
    let cancelled = false
    vi.spyOn(MutationObserver.prototype, "observe").mockImplementation(function (
      this: MutationObserver,
      target: Node,
      options?: MutationObserverInit,
    ) {
      originalObserve.call(this, target, options)
      if (target instanceof Element && target.closest("[data-reader-panel]") && !cancelled) {
        cancelled = true
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }))
      }
    })

    renderPanel({ kind: "settings", from: "menu" }, "reflowable")
    const slider = screen.getByRole("slider", { name: "Text size" })

    await waitFor(() => {
      expect(slider.style.display).not.toBe("none")
    })
    await Promise.resolve()

    expect(cancelled).toBe(true)
    expect(document.activeElement).not.toBe(slider)
  })
})
