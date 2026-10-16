### Added

- The sidebar lists your Jellyfin libraries under their Jellyfin names and in your Jellyfin order. Libraries you hide from My Media in Jellyfin stay hidden. A library added, removed or renamed on the server appears in the sidebar after the next sync, without a restart.
- Every library opens its own page. Movies and TV shows libraries keep every sort, filter and the A–Z rail, scoped to that library. Music, playlists, collections and other libraries open a grid served from your server with the sorts and filters that library supports.
- A title's back link and genre chips lead to the library it belongs to.
- While the first sync is still downloading your library, a movies or TV shows library page keeps its loading placeholder instead of saying there is nothing to show, also when Hide watched or another filter is on.

### Changed

- The Movies and Series entries in the sidebar are replaced by your libraries. Old Movies and Series links still open the matching titles across all libraries.
- Remembered filters are now kept per library. Filters remembered for the old Movies and Series pages are not carried over and reset once.

### Fixed

- Moving between pages no longer rewrites your account settings file when the remembered page has not changed.
