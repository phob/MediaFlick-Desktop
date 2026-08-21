import { useCallback, useState } from "react"

/**
 * An editable value that follows its source until the user changes it.
 *
 * Source changes are handled while rendering so consumers never commit a stale
 * draft and do not need an effect whose only job is to trigger another render.
 */
export function useSourceDraft<T>(source: T) {
  const [state, setState] = useState(() => ({ source, value: source }))

  if (!Object.is(state.source, source)) {
    setState({ source, value: source })
  }

  const value = Object.is(state.source, source) ? state.value : source
  const setValue = useCallback(
    (next: T) => setState({ source, value: next }),
    [source],
  )
  const updateValue = useCallback(
    (update: (current: T) => T) => {
      setState((current) => ({
        source,
        value: update(Object.is(current.source, source) ? current.value : source),
      }))
    },
    [source],
  )

  return [value, setValue, updateValue] as const
}
