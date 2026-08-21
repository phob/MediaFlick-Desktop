import { useCallback, useEffect, useState } from "react"

const SEARCH_DEBOUNCE_MS = 200
const MIN_SEARCH_LENGTH = 2

function normalizedSearchTerm(value: string) {
  const term = value.trim()
  return term.length >= MIN_SEARCH_LENGTH ? term : ""
}

/**
 * Keeps an editable search draft tied to its URL source while committing valid
 * terms after a short pause. An incomplete draft remains visible even though
 * its previously active URL search is cleared immediately.
 */
export function useLiveSearch(source: string, onCommit: (term: string) => void) {
  const [state, setState] = useState(() => ({ source, value: source }))

  if (state.source !== source) {
    const value = normalizedSearchTerm(state.value) === source ? state.value : source
    setState({ source, value })
  }

  const draft = state.source === source
    ? state.value
    : normalizedSearchTerm(state.value) === source
      ? state.value
      : source
  const setDraft = useCallback(
    (value: string) => setState({ source, value }),
    [source],
  )

  useEffect(() => {
    const next = normalizedSearchTerm(draft)
    if (next === source) return
    if (!next) {
      onCommit("")
      return
    }

    const timer = window.setTimeout(() => onCommit(next), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [draft, onCommit, source])

  const flush = useCallback(
    (value = draft) => {
      const next = normalizedSearchTerm(value)
      if (next !== source) onCommit(next)
    },
    [draft, onCommit, source],
  )

  return [draft, setDraft, flush] as const
}
