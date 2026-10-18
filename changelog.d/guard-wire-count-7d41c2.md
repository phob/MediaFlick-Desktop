### Changed

- The untyped-API ratchet now also counts routes that relay Companion JSON without a generated type, and Guard integrity treats its count and limits as guard code: editing them, or switching off their test, needs the `guards-changed` label, while lowering a limit still passes. CI also fails when the count's test is no longer compiled. The guard check's own tests run with the PR checks and `just check-pr`.

### Fixed

- `just verify collections` opens the Collections settings shelf again instead of the sidebar's Collections library.
