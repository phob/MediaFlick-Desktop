function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return true
  const mode = target.getAttribute("contenteditable")
  return target.isContentEditable || mode === "true" || mode === "" || mode === "plaintext-only"
}

const BROWSER_SHORTCUTS = new Set(["p", "s", "u", "r", "-", "=", "+", "0"])

export function installAppSurfaceGuard(target: Document = document): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
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

  const onWheel = (event: WheelEvent) => {
    if (event.ctrlKey) event.preventDefault()
  }

  const onCancelDrag = (event: DragEvent) => {
    event.preventDefault()
  }

  target.addEventListener("keydown", onKeyDown, true)
  target.addEventListener("wheel", onWheel, { passive: false, capture: true })
  for (const type of ["dragenter", "dragover", "drop"] as const) {
    target.addEventListener(type, onCancelDrag, true)
  }
  return () => {
    target.removeEventListener("keydown", onKeyDown, true)
    target.removeEventListener("wheel", onWheel, { capture: true })
    for (const type of ["dragenter", "dragover", "drop"] as const) {
      target.removeEventListener(type, onCancelDrag, true)
    }
  }
}
