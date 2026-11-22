### Fixed

- Enforced the exact chunk size limit on book byte-range responses instead of accepting one extra byte.
- On Linux, launching the external mpv player now strips MediaFlick's packaged Chromium library from `LD_PRELOAD` even when `LD_PRELOAD` contains non-UTF-8 paths, keeping all other preload entries intact.
