### Fixed

- Choosing **Settings** from the window context menu now navigates in-app instead of reloading the page, so discarding unsaved changes on another Settings shelf moves to Settings instead of closing or minimizing the app, and choosing **Settings** while already on the Player shelf keeps unsaved edits intact.
- Native `beforeunload` interception now emits `window-close-blocked` only when a window close is actually pending, preventing cancelled navigations or reloads from opening a close-confirmation dialog that calls `window.close()`.
