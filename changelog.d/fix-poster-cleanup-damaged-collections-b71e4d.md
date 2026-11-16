### Fixed

- Custom collection posters are no longer deleted after `collections.json` is found damaged. The damaged file is still moved aside as `collections.json.broken-<time>`, and while any such file remains in the config folder, the startup cleanup of unused posters is skipped, so the posters it may name stay on disk. Delete the `.broken` file once you no longer need it to let the cleanup run again.
