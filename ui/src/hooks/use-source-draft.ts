import { useCallback, useState } from "react"

function mergeDraft<T>(previous: T, draft: T, source: T): T {
  if (JSON.stringify(previous) === JSON.stringify(draft)) return source
  if (previous == null || draft == null || source == null) return source
  if (typeof previous !== "object" || typeof draft !== "object" || typeof source !== "object") return draft
  if (Array.isArray(draft)) return draft
  const next = { ...source }
  for (const key in source) {
    if (JSON.stringify(draft[key]) !== JSON.stringify(previous[key])) next[key] = draft[key]
  }
  return next
}

/** Refresh untouched fields while retaining edits; account changes start a new draft. */
export function useSourceDraft<T>(source: T, account?: string) {
  const [state, setState] = useState(() => ({ source, value: source, account }))

  const value = account !== state.account ? source : Object.is(state.source, source)
    ? state.value
    : mergeDraft(state.source, state.value, source)
  if (!Object.is(state.source, source) || account !== state.account) {
    setState({ source, value, account })
  }

  const setValue = useCallback(
    (next: T) => setState({ source, value: next, account }),
    [source, account],
  )
  const updateValue = useCallback(
    (update: (current: T) => T) => {
      setState((current) => ({
        source,
        account,
        value: update(current.account !== account ? source : mergeDraft(current.source, current.value, source)),
      }))
    },
    [source, account],
  )
  const acceptSaved = useCallback((saved: T, submitted: T) => {
    setState((current) => current.account === account
      ? { ...current, value: mergeDraft(submitted, current.value, saved) }
      : current)
  }, [account])

  return [value, setValue, updateValue, acceptSaved] as const
}
