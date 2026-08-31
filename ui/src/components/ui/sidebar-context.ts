import { createContext, useContext } from "react"

export type SidebarContextValue = {
  state: "expanded" | "collapsed"
  isMobile: boolean
}

export const SidebarContext = createContext<SidebarContextValue | null>(null)

export function useSidebar() {
  const context = useContext(SidebarContext)
  if (!context) {
    throw new Error("useSidebar must be used within a SidebarProvider.")
  }

  return context
}
