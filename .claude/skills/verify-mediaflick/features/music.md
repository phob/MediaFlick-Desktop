# Music and audiobooks

An album, track or audiobook (Music and Books libraries) opens the audio detail page: hero, then its tracks by disc. Music plays as a queue through the Built-in player or External mpv; the app keeps its pages and shows a now-playing bar instead of the video takeover. Audiobooks continue from their saved position; music starts at 0.

## Handles

- Album page: `{ role: "button", name: "Play" }`, `{ role: "button", name: "Shuffle" }` (two or more tracks), one row per track `{ role: "button", name: "Play <track>" }` in disc and track order inside `section[aria-label=Tracks]`, `{ role: "heading", name: "Disc N" }` when there are two or more discs. *Proven by `music`.*
- Audiobook page: `Continue at M:SS` and `From start` once Jellyfin holds a position, else `Play`. *Proven by `music`.*
- Now-playing bar: `Previous track`, `Pause` / `Resume`, `Next track` (disabled on the last entry), `Stop`, the timeline (an audiobook's chapters as ticks), `Volume N%`. It never hides. *Proven by `music`.*
- Keys, wherever focus is: MediaPlayPause, MediaTrackNext, MediaTrackPrevious, MediaStop (`xdotool key XF86AudioPlay` etc. on Linux). With focus on the page itself: Space pauses, ← 10 s back, → 30 s on. *Proven by `music`.*
- Play On: `ctx.remoteCommand("Playing", { playCommand: "PlayNow", itemIds, startIndex })`, then `Playing/PreviousTrack`, `Playing/NextTrack`, `Playing/Stop`. ItemIds play in the sender's order from StartIndex; an album id expands into its tracks. *Proven by `music`.*
- External mpv's NEXT and PREV keys send `script-message mediaflick-desktop next-track` / `previous-track` (read with `ctx.mpv("input-bindings")`). Audio opens no mpv window, so on Linux nothing can press them during audio; they matter where the OS hands media keys to mpv. *Bindings proven by `music`; a key press through them is unproven.*
- `ctx.player().nowPlaying`: `{ kind: "audio", mediaKind, current, next, playlistItemId, index, length, canPrevious, canNext, phase }`; `phase` is `preparing`, `loading` or `playing`. Video plays as `{ kind: "video" }`.

## Driving it

- `just verify music --account SERVER --timeout 600 --log-level "debug,jellyfin.playstate=trace,playback=trace"`, plus `--seed .claude/skills/verify-mediaflick/seeds/external-mpv` for External mpv.
- Fixtures on the server: the album "Test Tones" by "MediaFlick Fixtures" (2 discs, 5 tracks of 15 s: mp3, flac, opus, m4a, ogg) and the audiobook "Aesop's Fables" (.m4b, 6.3 h, 285 chapters).
- `ctx.audioSink()` first on Linux: without an audio device mpv cannot open an audio-only file.

## Gotchas

- Jellyfin 12.2 fills a session's `NowPlayingQueue` only from a Stopped report, so `/Sessions` shows the queue from the second track on; `PlaylistItemId` shows from the first. The reports themselves carry both every time (`app.log` at `jellyfin.playstate=trace`).
- mpv needs about 20 s to open "Aesop's Fables": ffmpeg reads its 285 chapter samples across the 93 MB file before `file-loaded` (1.4 s with `--demuxer-lavf-o=ignore_chapters=1`, measured with bare mpv on 2026-11-08).
- A key pressed while a track loads still reaches mpv; the bar reads the pause state mpv reports.
