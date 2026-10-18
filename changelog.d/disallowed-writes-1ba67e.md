### Changed

- Internal: Clippy now rejects plain `std::fs::write` calls and reads or writes of `settings.json` outside startup, so durable files keep going through the atomic save path.
