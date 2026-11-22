### Fixed

- Prevented the library identity sweep from deleting live titles when items shift or tie during pagination by adding a `SortName` tie-break (`DateCreated,SortName`) and skipping deletion when fewer unique titles are seen than the server reports.
