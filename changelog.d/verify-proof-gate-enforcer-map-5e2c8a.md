### Changed

- Pull-request checks and `just check-pr` now fail a verify feature-map claim that names a drive that does not exist, a run id, or evidence its drive does not write.
- `just verify <drive> --negative-control NAME` runs a drive on a build with its change reverted, saves the revert as `negative-control.patch`, and succeeds only when a check fails on an otherwise clean run. New drive checks record one.
- `AGENTS.md` lists every rule a check enforces next to that check, and adds the product decisions, supported Jellyfin versions, and working rules reviews kept asking for.
