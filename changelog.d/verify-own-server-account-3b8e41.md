### Added

- `just verify-login SERVER` signs in to your own Jellyfin server with Quick Connect, approved in Jellyfin, and caches that session outside the repository; `just verify <drive> --account SERVER` then starts verification runs signed in, with no password anywhere. Runs block Sign out, scan their evidence for the token, and put back what they changed on the server after the app exits. New drives cover sign-out and expiry, playback with the Built-in player and External mpv, Releases and ratings, Discover and Requests, Collections and the account settings.
