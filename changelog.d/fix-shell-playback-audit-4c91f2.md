### Fixed

- Launching MediaFlick a second time on Linux now focuses the running window instead of starting background services or opening a stray browser window. Simultaneous first launches on Windows now derive their single-instance mutex from the data directory path (Windows runtime verification is still outstanding).
- Shutdown log lines are now flushed to disk before MediaFlick exits, and background browser-shell posts are guarded once shutdown begins to reduce teardown races.
- Restarting MediaFlick while your server or library is slow to respond now reveals the loading screen immediately instead of leaving the window hidden.
- Reinstalling external mpv on Windows stops the running external mpv process before replacing its files (Windows runtime verification is still outstanding) and refreshes Player settings even if you left the Player page while the install finished.
- External mpv playback now ends cleanly at end-of-file even when `keep-open=yes` is set in your mpv configuration.
- Remote playback commands received during sign-out are now ignored, and switching accounts or signing back in immediately resets the Jellyfin live-update reconnect delay.
- Seeking backward during the first seconds of a resumed title now keeps your chosen position instead of snapping back to the resume point.
- Replacing an active video now notifies the interface cleanly without hiding the player bar or overlay, and stopping playback no longer lets a delayed status poll briefly reopen the player bar.
