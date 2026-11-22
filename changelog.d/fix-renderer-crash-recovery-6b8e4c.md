### Fixed

- MediaFlick now recovers automatically when the interface process crashes by revealing the window if it was still hidden, reloading the app, and showing an error screen with a retry option if crashes repeat instead of leaving a blank or hidden window.
- An unexpected interface rendering error now shows a recovery screen with a reload button instead of leaving the window hidden behind the loading cover.
- Startup now reveals the main window after a timeout if the interface does not finish initializing, so the window can no longer remain permanently hidden.
