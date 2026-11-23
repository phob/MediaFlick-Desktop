### Fixed

- Home settings now reconcile changes to libraries or genres that arrive while you have unsaved edits and retry once on a stale-list conflict instead of failing on every Save until Discard.
- Switching Player backend to external mpv no longer blocks Save on hidden built-in shortcut conflicts or hidden built-in shortcut settings.
- Collections settings now bind reorder, delete, and settings requests to the account open when Save started so an account switch mid-save cannot apply remaining edits to another account.
- Preference change notifications now apply in strict mutation order so an older update cannot overwrite a newer setting change.
- Viewing settings now validate language codes, conditional subtitle requirements, and numeric limits inline before saving.
- Active "Restart required" and settings recovery notices now remain visible after saving unrelated settings until resolved or restarted.
- Rapid Library filter changes now save in latest-wins order per account and page so an older request cannot overwrite a newer selection.
- Double-clicking a back link before navigation completes now pops only one history entry, and typing in search while a navigation update is pending no longer drops keystrokes.
