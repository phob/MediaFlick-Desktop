### Fixed

- MediaFlick Companion no longer keeps every collection result and identity mapping in memory indefinitely. Its caches now expire and have size limits.
- The Companion's ratings cache no longer rewrites its whole file after every change. Changes are saved together shortly afterwards and on shutdown, and expired ratings are removed from the file.
- Oversized responses from Seerr, Sonarr, or Radarr now fail with a clear error instead of being read into memory without limit.
