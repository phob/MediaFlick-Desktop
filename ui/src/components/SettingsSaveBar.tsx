import { AlertTriangle, Save, Undo2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useSettingsDraftGuard } from "@/lib/settings-drafts"

export default function SettingsSaveBar({ dirty, saving, saveDisabled, onSave, onDiscard, onReset, restartMessage }: {
  dirty: boolean
  saving: boolean
  saveDisabled?: boolean
  onSave: () => void
  onDiscard: () => void
  onReset: () => void
  restartMessage?: string
}) {
  useSettingsDraftGuard(dirty, saving)
  return (
    <div className="settings-save-bar" data-dirty={dirty}>
      <div className="flex min-w-0 items-center gap-2 text-sm">
        {restartMessage && <AlertTriangle className="size-4 shrink-0 text-primary" />}
        <span role="status">{restartMessage ?? (dirty ? "You have unsaved changes." : "")}</span>
      </div>
      <div className="flex shrink-0 gap-2">
        <Button variant="ghost" size="sm" onClick={onReset} disabled={saving}><Undo2 /> Reset</Button>
        <Button variant="outline" size="sm" onClick={onDiscard} disabled={saving || !dirty}>Discard</Button>
        <Button size="sm" onClick={onSave} disabled={saving || saveDisabled || !dirty}><Save /> {saving ? "Saving…" : "Save"}</Button>
      </div>
    </div>
  )
}
