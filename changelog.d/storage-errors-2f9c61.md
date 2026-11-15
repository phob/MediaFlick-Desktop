### Fixed

- A problem with the local library database is now reported as a local library failure. It was previously shown as an unexpected Jellyfin response.
- A database read error during library sync no longer makes the app download the whole library again; the sync stops and retries later.
- A "session expired" answer that reaches the background library sync after you switch accounts no longer signs out the new account.
- When MediaFlick cannot save the library list or a library's contents, the sync now stops and says "MediaFlick couldn't save your library" instead of carrying on as if that part were only not loaded yet.
- Home, collections, series Next Up, Seerr results and the Calendar now report a local library read failure as an error instead of showing empty shelves or missing "in your library" links.
- A collection refresh that cannot read the stored refresh state or your owned movies is skipped instead of refreshing on every run or saving results computed as if you owned nothing. A failed refresh no longer erases the time of the last successful one.
- When the server leaves out a library's item count, MediaFlick treats the count as unknown instead of 0: library progress no longer shows a total of 0, and a grid shows the items it received instead of an empty page.
