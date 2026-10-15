import { createContext, useContext } from "react"
import { useLocation } from "react-router-dom"
import { detailNavigationState } from "@/lib/navigation"

/** The name of the library page that is rendering, so its cards say where Back returns. */
export const LibraryPageName = createContext<string | null>(null)

export function useDetailNavigationState() {
  const location = useLocation()
  return detailNavigationState(location, useContext(LibraryPageName))
}
