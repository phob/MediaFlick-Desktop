import { useEffect, useRef, useState, type PointerEvent } from "react"

// Over macOS Screen Sharing every pointer move reaches the page as a leave of
// the window followed by an entry at the pointer's real position, although the
// pointer never left the sidebar. The entry followed within 49 ms in 99 % of
// 1,204 such pairs and within 160 ms in all of them (mediaflick-mac-test run
// 44769479069). The sidebar therefore collapses only once the pointer has
// stayed off it for longer than that.
const COLLAPSE_DELAY_MS = 250

/** Whether the pointer is over the hover sidebar, and the handlers that track it. */
export function useSidebarHover() {
  const [pointerIsOverSidebar, setPointerIsOverSidebar] = useState(false)
  const collapseTimer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(collapseTimer.current), [])

  return {
    pointerIsOverSidebar,
    onPointerEnter(event: PointerEvent<HTMLDivElement>) {
      // React also bubbles enter from the portalled user menu, whose unmount
      // under the pointer never sends the matching leave.
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
