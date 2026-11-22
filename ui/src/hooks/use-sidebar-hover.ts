import { useEffect, useRef, useState, type PointerEvent } from "react"

const COLLAPSE_DELAY_MS = 250

export function useSidebarHover() {
  const [pointerIsOverSidebar, setPointerIsOverSidebar] = useState(false)
  const collapseTimer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(collapseTimer.current), [])

  return {
    pointerIsOverSidebar,
    onPointerEnter(event: PointerEvent<HTMLDivElement>) {
      if (!(event.target instanceof Node && event.currentTarget.contains(event.target))) return
      window.clearTimeout(collapseTimer.current)
      setPointerIsOverSidebar(true)
    },
    onPointerLeave() {
      window.clearTimeout(collapseTimer.current)
      collapseTimer.current = window.setTimeout(() => setPointerIsOverSidebar(false), COLLAPSE_DELAY_MS)
    },
  }
}
