import { cleanup } from "@testing-library/react"
import { afterEach } from "vitest"

afterEach(() => cleanup())

// Radix feature-detects these browser APIs while setting up dismissable
// layers. jsdom does not currently provide them, but their no-op behavior is
// sufficient for interaction tests that do not measure or drag the dialog.
if (!(window as unknown as { PointerEvent?: unknown }).PointerEvent) {
  Object.defineProperty(window, "PointerEvent", { value: MouseEvent, writable: true })
}

if (!(window as unknown as { ResizeObserver?: unknown }).ResizeObserver) {
  Object.defineProperty(window, "ResizeObserver", {
    value: class ResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
    writable: true,
  })
}

const elementPrototype = HTMLElement.prototype as unknown as Record<string, unknown>
if (!elementPrototype.hasPointerCapture) {
  elementPrototype.hasPointerCapture = () => false
  elementPrototype.setPointerCapture = () => {}
  elementPrototype.releasePointerCapture = () => {}
}
