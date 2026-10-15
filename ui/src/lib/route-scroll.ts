import { createContext, useContext } from "react"

/** The route's scroll viewport, for content that scrolls with the page around it. */
export const RouteScrollContext = createContext<HTMLElement | null>(null)

export function useRouteScroller() {
  return useContext(RouteScrollContext)
}
