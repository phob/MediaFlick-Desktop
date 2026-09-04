import { useCallback, useState, type ReactNode } from "react"
import { useBeforeUnload, useBlocker } from "react-router-dom"
import { SettingsDraftsContext } from "@/lib/settings-drafts"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"

export default function SettingsDraftGuard({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<Map<string, boolean>>(() => new Map())
  const register = useCallback((id: string, dirty: boolean, saving: boolean) => {
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
  const blocker = useBlocker(({ currentLocation, nextLocation }) => dirty && currentLocation.pathname !== nextLocation.pathname)
  useBeforeUnload(useCallback((event) => {
    if (dirty) event.preventDefault()
  }, [dirty]))

  return <SettingsDraftsContext.Provider value={register}>
    {children}
    <Dialog open={blocker.state === "blocked"} onOpenChange={(open) => {
      if (!open && blocker.state === "blocked") blocker.reset()
    }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{saving ? "Saving settings…" : dirty ? "Leave without saving?" : "Leave settings?"}</DialogTitle>
          <DialogDescription>{saving ? "Wait for the save to finish before leaving this page." : dirty ? "Your unsaved settings will be discarded." : "There are no unsaved changes."}</DialogDescription>
        </DialogHeader>
        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => { if (blocker.state === "blocked") blocker.reset() }}>Keep editing</Button>
          <Button variant={dirty ? "destructive" : "default"} disabled={saving} onClick={() => { if (blocker.state === "blocked") blocker.proceed() }}>{dirty ? "Discard and leave" : "Leave page"}</Button>
        </div>
      </DialogContent>
    </Dialog>
  </SettingsDraftsContext.Provider>
}
