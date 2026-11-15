# Jellyfin public system info answers

Verbatim `GET /System/Info/Public` answers (with `Accept: application/json`), recorded on 2026-11-15. Tests read the server version from them, so each file is named for the release that answered.

| File | Server |
| --- | --- |
| `10.10.7.json` | Official portable Linux build `jellyfin_10.10.7-amd64.tar.xz` from repo.jellyfin.org, run on loopback |
| `10.11.0.json` | Official portable Linux build `jellyfin_10.11.0-amd64.tar.xz`, run on loopback |
| `10.11.11.json` | Official portable Linux build `jellyfin_10.11.11-amd64.tar.xz`, run on loopback |
| `12.4.0.json` | Jellyfin's demo server, `https://demo.jellyfin.org/stable` |
