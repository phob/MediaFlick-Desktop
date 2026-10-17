### Changed

- Every pull request now runs the UI, source-quality, Companion and Ubuntu build-and-test jobs before merge, plus Windows and macOS Clippy cross-checked from Ubuntu. The UI job also type-checks and builds the bundle and fails if pnpm rewrites the lockfile.
- New pull-request checks lint the workflows and fail a change that skips its changelog fragment (waived only by the `no-changelog` label), grows a source file past 1000 lines, or relaxes a lint, edits a check or raises a ratchet (accepted only with the `guards-changed` label). The guard check always runs from `main`, so a pull request cannot edit it away.
- `just check-pr` runs the same checks locally on Linux and macOS, and `just clippy-target <triple>` runs Clippy for another OS.
- Clippy now rejects `allow` attributes without a reason, `panic!` and `unreachable!` outside tests. The build script reports its failures without panicking.
