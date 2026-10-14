import { cleanup } from "@testing-library/react"
import { afterEach } from "vitest"

afterEach(() => cleanup())

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
