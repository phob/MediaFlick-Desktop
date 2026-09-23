### Fixed

- MediaFlick Companion now writes failures to the Jellyfin server log instead of hiding them. This covers unreachable or rejected Sonarr, Radarr, Seerr, MDBList, and TMDB connections, background ratings refreshes, ratings cache saves, and unreadable saved credentials. A lasting outage is logged once, with another entry when the service recovers. Log entries never include API keys, service addresses, or provider responses.
