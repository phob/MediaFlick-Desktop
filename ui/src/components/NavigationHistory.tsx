import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, type ReactNode } from "react"
import { createPath, Link, useLocation, useNavigate, useNavigationType, useResolvedPath, type LinkProps } from "react-router-dom"

const BackHistoryContext = createContext<(path: string) => boolean>(() => false)

export function NavigationHistory({ children }: { children: ReactNode }) {
  const location = useLocation()
  const action = useNavigationType()
  const navigate = useNavigate()
  const history = useRef<{
    entries: Array<{ key: string; path: string }>
    index: number
    pendingTargetKey: string | null
  }>({
    entries: [{ key: location.key, path: createPath(location) }],
    index: 0,
    pendingTargetKey: null,
  })

  useLayoutEffect(() => {
    const state = history.current
    state.pendingTargetKey = null
    if (state.entries[state.index]?.key === location.key) return
    const entry = { key: location.key, path: createPath(location) }
    const index = state.entries.findIndex((candidate) => candidate.key === entry.key)
    if (action === "POP" && index >= 0) state.index = index
    else if (action === "REPLACE") state.entries[state.index] = entry
    else if (action === "PUSH") {
      state.entries.splice(state.index + 1, Infinity, entry)
      state.index += 1
    } else {
      state.entries = [entry]
      state.index = 0
    }
  }, [location, action])

  const returnTo = useCallback((path: string) => {
    const state = history.current
    for (let target = state.index - 1; target >= 0; target -= 1) {
      const candidate = state.entries[target]
      if (candidate?.path === path) {
        if (state.pendingTargetKey !== null) return true
        state.pendingTargetKey = candidate.key
        void navigate(target - state.index)
        return true
      }
    }
    return false
  }, [navigate])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const back = event.key === "BrowserBack" || (event.altKey && event.key === "ArrowLeft")
      const forward = event.key === "BrowserForward" || (event.altKey && event.key === "ArrowRight")
      if ((!back && !forward) || event.ctrlKey || event.metaKey || event.shiftKey || event.defaultPrevented) return
      event.preventDefault()
      if (!event.repeat) void navigate(back ? -1 : 1)
    }
    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== 3 && event.button !== 4) return
      event.preventDefault()
      void navigate(event.button === 3 ? -1 : 1)
    }
    const cancelAuxiliary = (event: MouseEvent) => {
      if (event.button === 3 || event.button === 4) event.preventDefault()
    }
    window.addEventListener("keydown", onKeyDown)
    window.addEventListener("mousedown", cancelAuxiliary)
    window.addEventListener("mouseup", onMouseUp)
    window.addEventListener("auxclick", cancelAuxiliary)
    return () => {
      window.removeEventListener("keydown", onKeyDown)
      window.removeEventListener("mousedown", cancelAuxiliary)
      window.removeEventListener("mouseup", onMouseUp)
      window.removeEventListener("auxclick", cancelAuxiliary)
    }
  }, [navigate])

  return <BackHistoryContext value={returnTo}>{children}</BackHistoryContext>
}

export function BackLink({ to, onClick, ...props }: LinkProps) {
  const returnTo = useContext(BackHistoryContext)
  const path = createPath(useResolvedPath(to))
  return <Link {...props} to={to} onClick={(event) => {
    onClick?.(event)
    if (!event.defaultPrevented && event.button === 0 && !event.altKey && !event.ctrlKey
      && !event.metaKey && !event.shiftKey && (!props.target || props.target === "_self")
      && returnTo(path)) event.preventDefault()
  }} />
}
