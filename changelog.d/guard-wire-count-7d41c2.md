### Changed

- The guard check now also fails a pull request that edits the count behind the untyped wire ratchets, or turns its test off, unless the PR has the `guards-changed` label. The guard check's own tests run with the PR checks and `just check-pr`.

### Fixed

- `just verify collections` opens the Collections settings shelf again instead of the sidebar's Collections library.
