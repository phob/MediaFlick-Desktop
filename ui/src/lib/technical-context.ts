import { createContext, useContext, useEffect } from "react"
import type { MediaStream } from "./api"

export type TechnicalContextValue = {
  items: ReadonlyMap<string, MediaStream[]>
  register: (id: string) => () => void
}

export const TechnicalContext = createContext<TechnicalContextValue | null>(null)

export function useCardTechnical(itemId: string, visible = true) {
  const context = useContext(TechnicalContext)
  const register = context?.register
  useEffect(() => {
    if (!visible) return
    return register?.(itemId)
  }, [itemId, register, visible])
  return context?.items.get(itemId)
}
