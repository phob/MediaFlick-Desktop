### Fixed

- Opening Movie Franchises while franchise refresh is failing no longer retries in a tight loop or repeatedly re-runs failing scheduled collections. Failed automatic refreshes now back off and notify the view only when results or availability change, and resume immediately when Companion reconnects, gains franchise support, or is re-checked from Settings.
