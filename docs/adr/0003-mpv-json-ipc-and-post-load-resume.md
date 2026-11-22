# 0003. Every mpv runtime is driven over one JSON IPC path and resumes after `file-loaded`

Status: Accepted (2026-11-22). Decided in `6262c7af` (resume and IPC writer,
2026-06-12) and `3b0e5286` (one path for every runtime, 2026-09-23).

## Context

Resume and playstate reporting were debugged against real Jellyfin sessions
and external mpv on Windows. Several obvious approaches failed there. When
built-in libmpv arrived ([0001](0001-built-in-libmpv-player.md)), it could
either reuse that path or get its own control code.

## Decision

`MpvController` drives both runtimes the same way: libmpv starts mpv's own
`input-ipc-server`, and commands, observed properties, segment skipping,
Jellyfin reporting and next-episode handoff all use that JSON IPC. The libmpv
client API is used only to create, configure, initialize, poll and destroy the
handle.

IPC keeps a separate event reader and one persistent command writer per IPC
session, opened while mpv is idle and reused for every command. If the
transport fails or times out while built-in libmpv remains alive, MediaFlick
disconnects the poisoned worker and connects a fresh reader and persistent
command writer to the same `input-ipc-server` endpoint (with bounded reconnect
backoff and deferred `stop` delivery when playback was active) instead of
tearing down libmpv and its host window ([0001](0001-built-in-libmpv-player.md)).
Media loads without a start position; resume is a delayed absolute seek after
`file-loaded`, and the reported Jellyfin position is held until mpv reaches the
seek target. The operative rules are in [AGENTS.md](../../AGENTS.md#playback-invariants).

## Rejected alternatives

- Controlling libmpv through its client API (`mpv_command`,
  `mpv_observe_property`): a second control path for the same behavior, and
  the one without the real-session history.
- Resuming with `loadfile`'s `start` option or a URL `#t=` fragment: opened
  directly at the offset, external mpv on Windows could show a still frame
  until a later seek.
- Accepting mpv's position samples during the startup seek: mpv reports
  `time-pos=0` right after `file-loaded`, which reset Jellyfin's progress to
  zero.
- Sending `pause=false` at startup to compensate for load timing: mpv already
  reports unpaused, and the extra command caused IPC stalls.
- Opening a command pipe per command or after `file-loaded`, or writing through
  a clone of the event reader's handle: on Windows these hung with
  `mpv IPC write timed out`, or left mpv with an empty window because
  `loadfile` never arrived.

## Consequences

- A playback fix in the controller applies to both runtimes on every platform,
  and must be exercised on both.
- libmpv's own event queue is never read for playback, but it must still be
  drained, or it fills up.
- SVP on Windows connects to the fixed `\\.\pipe\mpvpipe`, so with SVP
  detected the built-in player uses that name for its IPC server instead of a
  unique one, and SVP shares MediaFlick's control channel.
