import { createContext, useContext, useEffect } from "react"
import type { MediaStream } from "./api"

export type TechnicalContextValue = {
  items: ReadonlyMap<string, MediaStream[]>
  register: (id: string) => () => void
}

export const TechnicalContext = createContext<TechnicalContextValue | null>(null)

/**
 * A visible card's live technical streams, or undefined until the batch that
 * carries them lands. Outside a `TechnicalProvider` this stays undefined and
 * the readout simply never appears.
 */
export function useCardTechnical(itemId: string, visible = true) {
  const context = useContext(TechnicalContext)
  const register = context?.register
  useEffect(() => {
    if (!visible) return
    return register?.(itemId)
  }, [itemId, register, visible])
  return context?.items.get(itemId)
}
