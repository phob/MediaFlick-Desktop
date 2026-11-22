import { useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { useBeforeUnload, useBlocker } from "react-router-dom"
import { SettingsDraftsContext } from "@/lib/settings-drafts"
import { readShellEvent } from "@/lib/shell-events"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"

function SettingsDraftGuardBoundary({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<Map<string, boolean>>(() => new Map())
  const discarders = useRef<Map<string, () => void>>(new Map())
  const register = useCallback((id: string, dirty: boolean, saving: boolean, onDiscard?: () => void) => {
    if (dirty && onDiscard) discarders.current.set(id, onDiscard)
    else discarders.current.delete(id)
    setPending((current) => {
      if (current.has(id) === (dirty || saving) && current.get(id) === (dirty || saving ? saving : undefined)) return current
      const next = new Map(current)
      if (dirty || saving) next.set(id, saving)
      else next.delete(id)
      return next
    })
  }, [])
  const dirty = pending.size > 0
  const saving = [...pending.values()].some(Boolean)
  const dirtyRef = useRef(dirty)
  useLayoutEffect(() => {
    dirtyRef.current = dirty
  }, [dirty])
  const blocker = useBlocker(({ currentLocation, nextLocation }) => dirty && currentLocation.pathname !== nextLocation.pathname)
  const [closeBlocked, setCloseBlocked] = useState(false)
  const [pendingAction, setPendingAction] = useState<(() => void) | null>(null)
  const leaving = useRef(false)
  const confirmAction = useCallback((action: () => void) => {
    if (dirtyRef.current) setPendingAction(() => action)
    else action()
  }, [])
  const controller = useMemo(() => ({ register, confirmAction }), [register, confirmAction])
  useBeforeUnload(useCallback((event) => {
    if (dirty && !leaving.current) event.preventDefault()
  }, [dirty]))
  useEffect(() => {
    const receive = (event: Event) => {
      if (readShellEvent(event)?.type === "window-close-blocked" && dirtyRef.current) setCloseBlocked(true)
    }
    window.addEventListener("mediaflick-desktop-shell", receive)
    return () => window.removeEventListener("mediaflick-desktop-shell", receive)
  }, [])
  const stay = () => {
    setCloseBlocked(false)
    setPendingAction(null)
    if (blocker.state === "blocked") blocker.reset()
  }
  const leave = () => {
    if (closeBlocked) {
      leaving.current = true
      window.close()
    } else if (pendingAction) {
      const action = pendingAction
      setPendingAction(null)
      for (const discard of [...discarders.current.values()]) discard()
      action()
    } else if (blocker.state === "blocked") {
      blocker.proceed()
    }
  }

  return <SettingsDraftsContext.Provider value={controller}>
    {children}
    <Dialog open={closeBlocked || pendingAction !== null || blocker.state === "blocked"} onOpenChange={(open) => {
      if (!open) stay()
    }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{saving ? "Saving settings…" : dirty ? "Leave without saving?" : "Leave settings?"}</DialogTitle>
          <DialogDescription>{saving ? "Wait for the save to finish before leaving this page." : dirty ? "Your unsaved settings will be discarded." : "There are no unsaved changes."}</DialogDescription>
        </DialogHeader>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={stay}>Keep editing</Button>
          <Button variant={dirty ? "destructive" : "default"} disabled={saving} onClick={leave}>{dirty ? "Discard and leave" : "Leave page"}</Button>
        </div>
      </DialogContent>
    </Dialog>
  </SettingsDraftsContext.Provider>
}

export default function SettingsDraftGuard({ children }: { children: ReactNode }) {
  const parent = useContext(SettingsDraftsContext)
  if (parent) return <>{children}</>
  return <SettingsDraftGuardBoundary>{children}</SettingsDraftGuardBoundary>
}
