### Fixed

- MediaFlick Companion no longer passes Seerr, Sonarr, or Radarr error text to Desktop or into its service status. Request and calendar errors now use fixed wording, so they can never reveal a service address or an echoed API key.
- Malformed collection requests and unexpected MDBList or TMDB data no longer make the Companion fail with a server error. A private flag it cannot read now hides the list instead of showing it.
- The Companion package version is now 0.2.2, following the published 0.2.1 test build.
