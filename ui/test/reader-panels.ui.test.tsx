import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { createRef, useState } from "react"
import { afterEach, describe, expect, test, vi } from "vitest"
import type { ReaderAppearance } from "@/lib/api"
import { nextLayer, type ReaderLayer } from "@/reader/model"
import { ReaderPanels, type PanelControl } from "@/reader/panels"

const DEFAULT_APPEARANCE: ReaderAppearance = {
  theme: "app",
  fontSize: 100,
  lineSpacing: "normal",
  margins: "normal",
  flow: "paginated",
  fit: "page",
}

function PanelHarness({
  initialLayer,
  layout = "reflowable",
}: {
  initialLayer: ReaderLayer
  layout?: "reflowable" | "fixed"
}) {
  const [layer, setLayer] = useState<ReaderLayer>(initialLayer)
  const control = createRef<PanelControl | null>()
  return (
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
      run={(command) => {
        if (command.kind === "open" || command.kind === "back") {
          const next = nextLayer(layer, command)
          if (next !== "leave") setLayer(next)
        }
      }}
      jump={() => {}}
      changeAppearance={() => {}}
      leave={() => {}}
    />
  )
}

function renderPanel(layer: ReaderLayer, layout: "reflowable" | "fixed" = "reflowable") {
  return render(<PanelHarness initialLayer={layer} layout={layout} />)
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

  test("moves focus to the Go to slider when opened from the reader menu", async () => {
    guardHiddenFocus()

    renderPanel({ kind: "menu", focus: "toc" }, "reflowable")
    const goToButton = screen.getByRole("button", { name: "Go to a position" })
    goToButton.focus()
    fireEvent.click(goToButton)

    const slider = await screen.findByRole("slider", { name: "Position in the book" })
    await waitFor(() => {
      expect(document.activeElement).toBe(slider)
    })
  })

  test("moves focus to the Appearance slider when opened from the reader menu", async () => {
    guardHiddenFocus()

    renderPanel({ kind: "menu", focus: "toc" }, "reflowable")
    const appearanceButton = screen.getByRole("button", { name: "Appearance" })
    appearanceButton.focus()
    fireEvent.click(appearanceButton)

    const slider = await screen.findByRole("slider", { name: "Text size" })
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
})
