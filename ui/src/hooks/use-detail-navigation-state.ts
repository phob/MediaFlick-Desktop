import { useLocation } from "react-router-dom"
import { detailNavigationState } from "@/lib/navigation"
import { useCachedLibraries } from "@/lib/queries"

export function useDetailNavigationState() {
  return detailNavigationState(useLocation(), useCachedLibraries())
}
