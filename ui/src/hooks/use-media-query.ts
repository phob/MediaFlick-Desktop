import { useCallback, useSyncExternalStore } from "react"

export function useMediaQuery(query: string) {
  const subscribe = useCallback((onChange: () => void) => {
    const media = globalThis.window?.matchMedia?.(query)
    if (!media) return () => undefined
    media.addEventListener("change", onChange)
    return () => media.removeEventListener("change", onChange)
  }, [query])
  const snapshot = useCallback(
    () => globalThis.window?.matchMedia?.(query).matches === true,
    [query],
  )
  return useSyncExternalStore(subscribe, snapshot, () => false)
}
