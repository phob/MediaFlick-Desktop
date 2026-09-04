import { createContext, useContext, useEffect, useId } from "react"

export const SettingsDraftsContext = createContext<((id: string, dirty: boolean, saving: boolean) => void) | null>(null)

export function useSettingsDraftGuard(dirty: boolean, saving: boolean) {
  const register = useContext(SettingsDraftsContext)
  const id = useId()
  useEffect(() => {
    register?.(id, dirty, saving)
    return () => register?.(id, false, false)
  }, [id, dirty, saving, register])
}
