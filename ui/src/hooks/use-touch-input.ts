import { useEffect, useState } from "react"

export const TOUCH_INPUT_QUERY = "(hover: none), (pointer: coarse), (any-pointer: coarse)"

function matchesTouchInput() {
  return typeof window !== "undefined" && window.matchMedia?.(TOUCH_INPUT_QUERY).matches === true
}

/** Prefer a non-nested sheet when the primary input cannot reliably hover. */
export function useTouchInput() {
  const [touchInput, setTouchInput] = useState(matchesTouchInput)

  useEffect(() => {
    const query = window.matchMedia?.(TOUCH_INPUT_QUERY)
    if (!query) return
    const update = () => setTouchInput(query.matches)
    update()
    query.addEventListener("change", update)
    return () => query.removeEventListener("change", update)
  }, [])

  return touchInput
}
