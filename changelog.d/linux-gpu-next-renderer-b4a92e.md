### Changed

- Linux's built-in player now renders video and browser controls through gpu-next, preferring Vulkan with an OpenGL fallback. The existing X11/XWayland window, browser scaling, playback controls, and delayed resume behavior are retained.
- Video rendering now follows X11 drawable dimensions, which can increase GPU work on fractionally scaled XWayland desktops.
