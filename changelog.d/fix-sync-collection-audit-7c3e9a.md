### Fixed

- Collections set to Manual refresh now populate automatically after a cache wipe or recreation (while still backing off on repeated failures), and opening a collection detail page while results are still loading now offers a Retry button or reports provider unavailability instead of staying stuck on "Updating results".
- Quitting MediaFlick during a library sync no longer marks the library as failed on the next launch, so collections remain available immediately at startup.
- Scheduled collection refreshes no longer run duplicate updates for the same collection at the same time, and no longer save stale snapshots or failure records if a collection is deleted or the account changes while a refresh is in flight.
- Large incremental library syncs that hit the per-cycle page limit now keep the previous sync cursor and schedule a full catalog refresh on the next cycle so no items past the cap are skipped.
- The Home screen's "Because you watched" row no longer stays missing for the rest of the session if Home loads before the first library sync page finishes.
- Library sync progress now sets the active phase before listing libraries and resets between sync cycles instead of briefly showing the previous cycle's phase.
