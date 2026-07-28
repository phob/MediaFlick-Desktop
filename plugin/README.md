# MediaFlick Companion

The server-side companion for MediaFlick Desktop. It exposes typed,
Jellyfin-authenticated endpoints under `/MediaFlick`, keeps Sonarr, Radarr, and
Seerr API keys on the server, and runs Seerr calls as the mapped Jellyfin user.

The plugin targets Jellyfin 10.11.11 (`net9.0`). Build it with `just plugin`,
test it with `just plugin-test`, and deploy the release publish output to the
configured development server with `just plugin-deploy`.
