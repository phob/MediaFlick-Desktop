### Fixed

- Collections settings now rebase a dirty collection reorder draft when collections are added or removed so newly created collections stay visible and saving persists the complete order without returning a `400` error.
- Collections settings now run collection reordering before destructive profile deletions and settings updates, and clear committed deletes and reorders from the draft after partial save failures so already-deleted collections do not remain stuck as unsaved changes.
