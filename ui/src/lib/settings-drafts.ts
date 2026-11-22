import { createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useRef } from "react"

export type GuardedActionCopy = {
  dirtyTitle: string
  cleanTitle: string
  savingDescription: string
  discardLabel: string
  proceedLabel: string
}

export type SettingsDraftsController = {
  register: (id: string, dirty: boolean, saving: boolean, onDiscard?: () => void) => void
  confirmAction: (action: () => void, copy: GuardedActionCopy) => void
}

export const SettingsDraftsContext = createContext<SettingsDraftsController | null>(null)

export function useSettingsDraftGuard(dirty: boolean, saving: boolean, onDiscard?: () => void) {
  const controller = useContext(SettingsDraftsContext)
  const id = useId()
  const onDiscardRef = useRef(onDiscard)
  useLayoutEffect(() => {
    onDiscardRef.current = onDiscard
  }, [onDiscard])
  const discard = useCallback(() => {
    onDiscardRef.current?.()
  }, [])
  useEffect(() => {
    controller?.register(id, dirty, saving, discard)
    return () => controller?.register(id, false, false)
  }, [id, dirty, saving, discard, controller])
}

export function useGuardedAction() {
  const controller = useContext(SettingsDraftsContext)
  return useCallback(
    (action: () => void, copy: GuardedActionCopy) => {
      if (controller) controller.confirmAction(action, copy)
      else action()
    },
    [controller],
  )
}
