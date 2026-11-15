### Removed

- Removed repairs for caches written by earlier builds. A library cache that has items but no catalog-ready marker, and whose first sync has not finished, is no longer treated as ready, and Seerr images cached as error responses are refetched instead of deleted.
- Companion ratings now name each source once, as `sourceId`, instead of repeating it as `source` and `rawSource`.
