### Fixed

- Switching Jellyfin accounts or signing out while a request is still running no longer affects the next account. A late "session expired" answer to the previous account no longer signs the new one out. A title the previous account found missing is no longer removed from the new account's library. A watched or My List change no longer lands in the wrong account's library. Remote-control play requests from a previous account are ignored.
