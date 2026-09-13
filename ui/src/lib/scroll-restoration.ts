import { useLayoutEffect } from "react"
import { useLocation, useNavigationType } from "react-router-dom"

const positions = new Map<string, number>()

/** Restore only history traversal; new routes and changed filters start at the top. */
export function useScrollRestoration(element: HTMLElement | null, scope: string, ready = true) {
  const location = useLocation()
  const navigationType = useNavigationType()
  const key = JSON.stringify([location.key, scope])

  useLayoutEffect(() => {
    if (!element || !ready) return
    const top = navigationType === "POP" ? positions.get(key) ?? 0 : 0
    let restoring = top > 0
    const restore = () => {
      element.scrollTo({ top, behavior: "instant" })
      restoring = Math.abs(element.scrollTop - top) > 1
    }
    restore()
    // Async queries and virtual grids can initially be shorter than the saved
    // offset. Retry as content arrives, until it fits or the user takes over.
    const retry = () => {
      if (!restoring) return
      restore()
      if (!restoring) {
        resize.disconnect()
        mutations.disconnect()
      }
    }
    const resize = new ResizeObserver(retry)
    const observeContent = () => {
      if (!restoring) return
      resize.disconnect()
      resize.observe(element)
      for (const child of element.children) resize.observe(child)
      retry()
    }
    const mutations = new MutationObserver(observeContent)
    if (restoring) {
      mutations.observe(element, { childList: true, subtree: true, attributes: true })
      observeContent()
    }
    const save = () => { if (!restoring) positions.set(key, element.scrollTop) }
    const takeOver = () => {
      restoring = false
      resize.disconnect()
      mutations.disconnect()
    }
    element.addEventListener("scroll", save)
    element.addEventListener("wheel", takeOver, { passive: true })
    element.addEventListener("pointerdown", takeOver)
    element.addEventListener("keydown", takeOver)
    return () => {
      // Save on scroll, not unmount: React may already have removed the tall
      // content and the browser may have clamped scrollTop back to zero.
      mutations.disconnect()
      resize.disconnect()
      element.removeEventListener("scroll", save)
      element.removeEventListener("wheel", takeOver)
      element.removeEventListener("pointerdown", takeOver)
      element.removeEventListener("keydown", takeOver)
    }
  }, [element, key, navigationType, ready])
}
