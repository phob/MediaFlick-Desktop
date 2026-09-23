### Changed

- The local API handlers now share one early-exit path for invalid requests, signed-out sessions and account switches, replacing about 80 hand-written copies. Responses are unchanged.
