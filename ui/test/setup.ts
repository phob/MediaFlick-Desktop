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

function isElementContentEditable(element: HTMLElement): boolean {
  if (element.ownerDocument.designMode === "on") return true
  let current: HTMLElement | null = element
  while (current) {
    const value = current.getAttribute("contenteditable")?.toLowerCase()
    if (value === "" || value === "true" || value === "plaintext-only") return true
    if (value === "false") return false
    current = current.parentElement
  }
  return false
}

if (!("isContentEditable" in HTMLElement.prototype)) {
  Object.defineProperty(HTMLElement.prototype, "isContentEditable", {
    configurable: true,
    get(this: HTMLElement): boolean {
      return isElementContentEditable(this)
    },
  })
}
