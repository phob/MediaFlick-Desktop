// The expanded-card layer's context, kept apart from the component that renders
// it so that file exports components and nothing else — which is what lets Vite
// hot-replace the panel without dropping the provider's state.

import { createContext, useContext } from "react"
import type React from "react"
import type { ItemSummary } from "./api"

export interface PreviewTarget {
  item: ItemSummary
  rect: DOMRect
}

export interface PreviewApi {
  /** Arms the open timer for `item`, anchored on the element the pointer entered. */
  open: (item: ItemSummary, anchor: HTMLElement) => void
  /** Disarms a pending open — the pointer left before the delay elapsed. */
  cancel: () => void
  /** Arms the close timer; `hold` cancels it again. */
  release: () => void
  hold: () => void
  /** The id currently expanded, so the card underneath can suppress its hover. */
  activeId: string | null
}

export const PreviewContext = createContext<PreviewApi | null>(null)

interface PreviewHandlers {
  onPointerEnter?: (event: React.PointerEvent<HTMLElement>) => void
  onPointerLeave?: (event: React.PointerEvent<HTMLElement>) => void
}

interface PreviewState {
  expanded: boolean
  handlers: PreviewHandlers
}

/**
 * Hover handlers for one card, plus whether that card is the expanded one.
 *
 * Outside a `PreviewProvider` — or with `enabled` off — this hands back empty
 * handlers rather than throwing, so a card can be rendered anywhere without the
 * caller having to know whether the layer is mounted above it.
 */
export function usePreview(
  item: ItemSummary,
  enabled = true,
): PreviewState {
  const preview = useContext(PreviewContext)
  if (!preview || !enabled) return { handlers: {}, expanded: false }

  return {
    expanded: preview.activeId === item.id,
    handlers: {
      // Pointer, not mouse: a touch drag across a rail would otherwise arm the
      // timer for every card it passed and pop a panel nothing can dismiss.
      onPointerEnter: (event) => {
        if (event.pointerType !== "mouse") return
        // A pointer arriving with a button already held is mid-drag — a text
        // selection or a press that wandered. A panel opened under it would
        // read the eventual release as a click it was never meant to receive.
        if (event.buttons !== 0) return
        preview.open(item, event.currentTarget)
      },
      onPointerLeave: (event) => {
        if (event.pointerType !== "mouse") return
        preview.cancel()
      },
    },
  }
}
