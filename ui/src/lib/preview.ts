import { createContext, useContext, useSyncExternalStore } from "react"
import type React from "react"
import type { ItemSummary } from "./api"

export interface PreviewTarget {
  item: ItemSummary
  rect: DOMRect
}

export interface PreviewApi {
  open: (item: ItemSummary, anchor: HTMLElement) => void
  cancel: () => void
  release: () => void
  hold: () => void
  expanded: ExpandedStore
  enabled: boolean
}

export interface ExpandedStore {
  current: () => string | null
  set: (id: string | null) => void
  subscribe: (listener: () => void) => () => void
}

export function createExpandedStore(): ExpandedStore {
  let current: string | null = null
  const listeners = new Set<() => void>()
  return {
    current: () => current,
    set: (id) => {
      if (id === current) return
      current = id
      for (const listener of listeners) listener()
    },
    subscribe: (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
  }
}

const DETACHED_STORE = createExpandedStore()

export const PreviewContext = createContext<PreviewApi | null>(null)

interface PreviewHandlers {
  onPointerEnter?: (event: React.PointerEvent<HTMLElement>) => void
  onPointerLeave?: (event: React.PointerEvent<HTMLElement>) => void
}

interface PreviewState {
  expanded: boolean
  handlers: PreviewHandlers
  previewsEnabled: boolean
}

export function usePreview(
  item: ItemSummary,
  enabled = true,
): PreviewState {
  const preview = useContext(PreviewContext)
  const store = preview?.expanded ?? DETACHED_STORE
  const expanded = useSyncExternalStore(store.subscribe, () => store.current() === item.id)
  if (!preview) return { handlers: {}, expanded: false, previewsEnabled: true }
  if (!preview.enabled || !enabled) {
    return { handlers: {}, expanded: false, previewsEnabled: preview.enabled }
  }

  return {
    expanded,
    previewsEnabled: preview.enabled,
    handlers: {
      onPointerEnter: (event) => {
        if (event.pointerType !== "mouse") return
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
