### Fixed

- Custom collection posters are no longer deleted when `collections.json` is damaged and has no valid backup. On that start the file is moved aside as before, and the cleanup of unused posters is skipped, so the posters it names stay on disk.
