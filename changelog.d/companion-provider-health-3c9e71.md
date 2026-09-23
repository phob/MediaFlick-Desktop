### Fixed

- A TMDB timeout or server error during credential validation no longer marks a saved TMDB key as rejected in MediaFlick Companion. Only a real authentication failure does, and TMDB outages are no longer described as MDBList outages.
- MDBList and TMDB now follow the same Companion retry and credential-health rules for ratings and collections, including exponential backoff after outages.
- IMDb IDs are checked the same way for ratings, identity mapping, and Seerr details.
- A missing Seerr title or a per-user Seerr permission answer no longer shows Seerr as unavailable in Desktop's Companion settings.
