### Changed

- Each library sync now also reads which Jellyfin libraries you can see and which movies and shows each one holds. This prepares browsing every library under its own name; the app looks the same for now. The first launch after updating rebuilds the local catalog once.
- `/api/items`, which the app window uses to list titles, now ignores an invalid `favorite` value instead of reading it as "not a favorite".
