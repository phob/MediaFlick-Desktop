### Fixed

- A problem with the local library database is now reported as a local library failure. It was previously shown as an unexpected Jellyfin response.
- A database read error during library sync no longer makes the app download the whole library again; the sync stops and retries later.
- A "session expired" answer that reaches the background library sync after you switch accounts no longer signs out the new account.
