### Fixed

- When MediaFlick cannot open its library at startup, for example because a newer MediaFlick wrote `library.db` or a schema update failed and was rolled back, the window now shows a "MediaFlick could not start" screen with the reason instead of the sign-in screen. The file is still left as it was.
- A failed schema update now names the step it failed on (from schema X to Y) and SQLite's error, without the statement text that made the message several kilobytes long; the log keeps the full error.
