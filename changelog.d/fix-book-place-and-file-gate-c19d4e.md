### Fixed

- A book's place that could not reach Jellyfin is no longer lost when the book was already started on another device: reopening it keeps the newer page as long as Jellyfin still holds what it held when the book was opened.
- The reader's file and position routes now serve only a book the reader opened, so they no longer read or write any other item's file or resume point.
