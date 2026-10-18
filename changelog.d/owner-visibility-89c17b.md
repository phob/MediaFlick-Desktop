### Changed

- Internal: the services locator and the settings file reader each have a single owner, so lower layers can no longer reach the process-wide services or read and write `settings.json` themselves.
