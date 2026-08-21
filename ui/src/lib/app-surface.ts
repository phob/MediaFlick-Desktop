function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return true
  // jsdom never reports isContentEditable, so the attribute decides as well.
  const mode = target.getAttribute("contenteditable")
  return target.isContentEditable || mode === "true" || mode === "" || mode === "plaintext-only"
}

// Accelerators whose browser defaults have no meaning in an app shell: print,
// save, and view-source act on a "document" that does not exist here, reload
// discards the session view, and zoom rescales chrome the OS already sizes.
const BROWSER_SHORTCUTS = new Set(["p", "s", "u", "r", "-", "=", "+", "0"])

export function installAppSurfaceGuard(): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
    // F7 toggles Chromium caret browsing, which would open keyboard-driven
    // selection; the shell has no document to place a caret in.
    if (event.key === "F7" || event.key === "F5") {
      event.preventDefault()
      return
    }
    if (!(event.ctrlKey || event.metaKey)) return
    const key = event.key.toLowerCase()
    if (key === "a" && !isEditableTarget(event.target)) {
      event.preventDefault()
    } else if (BROWSER_SHORTCUTS.has(key)) {
      event.preventDefault()
    }
  }

  // Trackpad pinch synthesizes a ctrl+wheel sequence, so this pins pinch zoom
  // along with ctrl+wheel. The listener must be non-passive to matter.
  const onWheel = (event: WheelEvent) => {
    if (event.ctrlKey) event.preventDefault()
  }

  // A drop the page does not claim navigates the frame to the dropped file or
  // URL; canceling the drag sequence keeps the shell on its own scheme.
  const onCancelDrag = (event: DragEvent) => {
    event.preventDefault()
  }

  document.addEventListener("keydown", onKeyDown, true)
  document.addEventListener("wheel", onWheel, { passive: false, capture: true })
  for (const type of ["dragenter", "dragover", "drop"] as const) {
    document.addEventListener(type, onCancelDrag, true)
  }
  return () => {
    document.removeEventListener("keydown", onKeyDown, true)
    document.removeEventListener("wheel", onWheel, { capture: true })
    for (const type of ["dragenter", "dragover", "drop"] as const) {
      document.removeEventListener(type, onCancelDrag, true)
    }
  }
}
