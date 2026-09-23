### Fixed

- A garbled or incomplete request from the app window is now refused instead of being read with default values. For example, a damaged "mark as unwatched" request can no longer mark the item as watched, and a player command with a missing value is no longer sent to the player.
