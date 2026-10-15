### Fixed

- A playlist's page lists its items in the playlist's own order instead of by name, and shows an item the playlist holds twice in both places.
- Album, playlist and folder pages that are open refresh when they change on the server, for example after items are added to or reordered in a playlist on another device. Jellyfin reports such changes about 30 seconds after the server goes quiet.
- When Jellyfin stops giving you any movie or TV show while an empty movies or TV shows library is still listed, for example after you lose access to your other libraries, the cached titles disappear once two identity sweeps in a row, at least 10 minutes and at most 2 hours apart, find no titles and the server counts zero, instead of staying until a full re-sync.
