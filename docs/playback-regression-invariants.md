# Playback Regression Invariants

This file documents playback behavior that has already been debugged against
real Jellyfin Web plus external mpv logs. Treat these as higher-priority
invariants when changing playback, bridge, or IPC code.

## Resume Playback

- Do not pass Jellyfin resume positions to mpv through `loadfile` option
  `start` or through a media URL `#t=` fragment.
- Resumed media must load the stream URL without the fragment, report the
  intended resume position to Jellyfin, then send a delayed absolute seek after
  mpv emits `file-loaded`.
- While that startup seek is pending, ignore transient mpv position samples far
  below the resume target. mpv commonly reports `time-pos=0.0` immediately after
  `file-loaded`; accepting that sample resets Jellyfin/Web progress to zero.
- Starting media from zero must not send a startup seek or startup unpause
  command. mpv already reports `pause=false`; an extra startup unpause caused
  IPC stalls.

## mpv IPC On Windows

- Keep event reading and command writing on separate pipe connections.
- Open the command writer while mpv is still idle, before `loadfile`, and reuse
  it for `loadfile`, startup seek, web controls, and later commands.
- Do not open a fresh command pipe after `file-loaded`; this was observed to
  hang and produce `mpv IPC write timed out`.
- Do not write commands through a clone of the event-reader handle; that was
  observed to leave mpv with an empty window because `loadfile` did not reach it.

## Verification

Before accepting playback changes, verify all three flows:

- Start a movie from the beginning.
- Resume an in-progress movie.
- Use continuous play/resume from Jellyfin Web.

The expected resume log shape is:

- `loaded Jellyfin stream in mpv`
- `activating pending playback`
- `queued mpv startup seek after file load`
- `sending delayed mpv startup seek`
- `sent mpv command`
- mpv reports `time-pos` near the resume target

