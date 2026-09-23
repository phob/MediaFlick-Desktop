### Changed

- MediaFlick Companion now builds every Seerr answer for Desktop from fixed, typed fields instead of passing parts of Seerr's JSON through. Unused `configured`, `expired`, and `serverUrl` status fields left over from Desktop's old direct Seerr connection are no longer sent.

### Fixed

- A Jellyfin user who has not been imported into Seerr no longer makes the Companion scan every Seerr user on each request. The result is remembered for two minutes, and simultaneous first requests share one lookup.
- Seerr movie details with revenue above about two billion now show the revenue instead of leaving it blank.
