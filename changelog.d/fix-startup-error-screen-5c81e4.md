### Fixed

- When MediaFlick cannot open its library at startup, for example because a newer MediaFlick wrote `library.db` or a schema update failed and was rolled back, the window now shows a "MediaFlick could not start" screen with the reason instead of the sign-in screen. The file is still left as it was.
