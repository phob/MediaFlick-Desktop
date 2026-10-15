import { createContext, useContext } from "react"

export const RouteScrollContext = createContext<HTMLElement | null>(null)

export function useRouteScroller() {
  return useContext(RouteScrollContext)
}
