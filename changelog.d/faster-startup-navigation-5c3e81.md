### Changed

- The app opens sooner. It loads the library and account data while the browser engine starts, requests everything the first screen needs at once, and shows the window as soon as that screen is ready.
- The first background library sync after launch now waits until Home has appeared, so Home no longer reloads partway through opening.
- Series pages show seasons and episodes from the local library immediately and check Jellyfin for changes in the background. A series with a single season no longer waits for Next Up before showing its episodes.
- Library sync progress and hover previews no longer redraw the rest of the app, which keeps browsing smooth while a sync runs.
