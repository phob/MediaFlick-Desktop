### Fixed

- MediaFlick Desktop now starts on macOS. Every earlier macOS build, 0.2.0 included, crashed at startup before a window opened, because the app never loaded the Chromium Embedded Framework. The app now loads it and ships the helper apps that Chromium runs its processes in. Cmd+Q, the app menu's Quit and the Dock's Quit now close MediaFlick the way the window's Exit does, so it saves the playback position and settings first.
