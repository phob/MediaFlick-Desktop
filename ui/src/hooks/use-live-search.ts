import { useCallback, useEffect, useState } from "react"

const SEARCH_DEBOUNCE_MS = 200
const MIN_SEARCH_LENGTH = 2
const MAX_PENDING_COMMITS = 8

type LiveSearchState = {
  source: string
  value: string
  pendingCommits: readonly string[]
}

function normalizedSearchTerm(value: string) {
  const term = value.trim()
  return term.length >= MIN_SEARCH_LENGTH ? term : ""
}

function appendPendingCommit(pendingCommits: readonly string[], term: string): readonly string[] {
  return pendingCommits.at(-1) === term
    ? pendingCommits
    : [...pendingCommits.slice(-(MAX_PENDING_COMMITS - 1)), term]
}

function reconcileLiveSearchState(state: LiveSearchState, source: string): LiveSearchState {
  if (state.source === source) return state
  const commitIndex = state.pendingCommits.indexOf(source)
  if (commitIndex >= 0) {
    return {
      source,
      value: state.value,
      pendingCommits: state.pendingCommits.slice(commitIndex + 1),
    }
  }
  const value = normalizedSearchTerm(state.value) === source ? state.value : source
  return { source, value, pendingCommits: [] }
}

export function useLiveSearch(source: string, onCommit: (term: string) => void) {
  const [state, setState] = useState<LiveSearchState>(() => ({
    source,
    value: source,
    pendingCommits: [],
  }))

  const reconciled = reconcileLiveSearchState(state, source)
  if (reconciled !== state) {
    setState(reconciled)
  }

  const draft = reconciled.value
  const setDraft = useCallback(
    (value: string) => setState((current) => {
      const next = reconcileLiveSearchState(current, source)
      const clearing = !normalizedSearchTerm(value) && source !== ""
      return {
        ...next,
        value,
        pendingCommits: clearing ? appendPendingCommit(next.pendingCommits, "") : next.pendingCommits,
      }
    }),
    [source],
  )

  const commit = useCallback(
    (term: string) => {
      setState((current) => {
        const next = reconcileLiveSearchState(current, source)
        return { ...next, pendingCommits: appendPendingCommit(next.pendingCommits, term) }
      })
      onCommit(term)
    },
    [onCommit, source],
  )

  useEffect(() => {
    const next = normalizedSearchTerm(draft)
    if (next === source) return
    if (!next) {
      onCommit("")
      return
    }

    const timer = window.setTimeout(() => commit(next), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [commit, draft, onCommit, source])

  const flush = useCallback(
    (value = draft) => {
      const next = normalizedSearchTerm(value)
      if (next !== source) commit(next)
    },
    [commit, draft, source],
  )

  return [draft, setDraft, flush] as const
}
