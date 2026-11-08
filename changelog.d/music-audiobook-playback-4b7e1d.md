### Added

- Albums, tracks and audiobooks play as a queue through the Built-in player and External mpv. Tracks advance on their own at the end of each one, Next and Previous move through the queue (Previous restarts a track after its first 3 seconds), and Jellyfin sees the whole queue and the current entry in every playback report. Audiobooks continue from their saved position; music always starts at the beginning.

### Changed

- Audio never opens a player window, never takes the default fullscreen and never raises External mpv, and a right click pauses only video. Signing out or switching accounts stops an audio queue.
