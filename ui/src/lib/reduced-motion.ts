import { useEffect, useState } from "react"

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)"

function systemPrefersReducedMotion() {
  return globalThis.window?.matchMedia?.(REDUCED_MOTION_QUERY).matches === true
}

/** Reactive operating-system motion preference shared by runtime and previews. */
export function usePrefersReducedMotion() {
  const [reduced, setReduced] = useState(systemPrefersReducedMotion)

  useEffect(() => {
    const query = window.matchMedia?.(REDUCED_MOTION_QUERY)
    if (!query) return
    const update = () => setReduced(query.matches)
    update()
    query.addEventListener("change", update)
    return () => query.removeEventListener("change", update)
  }, [])

  return reduced
}
