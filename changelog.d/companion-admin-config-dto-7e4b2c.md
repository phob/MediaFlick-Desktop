### Changed

- Updated MediaFlick Companion's `GET /MediaFlick/admin/config` endpoint to return a typed response record serialized with `CompanionJson.CamelCase` instead of a manually lowercased anonymous object, preserving the redacted camelCase JSON payload.
