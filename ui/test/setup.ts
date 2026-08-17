import { cleanup } from "@testing-library/react"
import { afterEach } from "vitest"

afterEach(() => cleanup())

// Radix feature-detects these browser APIs while setting up dismissable
// layers. jsdom does not currently provide them, but their no-op behavior is
// sufficient for interaction tests that do not measure or drag the dialog.
if (!window.PointerEvent) {
  Object.defineProperty(window, "PointerEvent", { value: MouseEvent, writable: true })
}

if (!globalThis.ResizeObserver) {
  Object.defineProperty(window, "ResizeObserver", {
    value: class ResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
    writable: true,
  })
}

if (!HTMLElement.prototype.hasPointerCapture) {
  HTMLElement.prototype.hasPointerCapture = () => false
  HTMLElement.prototype.setPointerCapture = () => {}
  HTMLElement.prototype.releasePointerCapture = () => {}
}

if (!HTMLElement.prototype.scrollIntoView) {
  HTMLElement.prototype.scrollIntoView = () => {}
}
