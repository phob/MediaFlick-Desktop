// Music and audiobooks through the user path, against your own server with the
// fixtures named below. Writes the test user's playstate and puts it back.
//   just verify music --account SERVER --timeout 600 --log-level "debug,jellyfin.playstate=trace,playback=trace"
//   (External mpv: add --seed .claude/skills/verify-mediaflick/seeds/external-mpv)
import { execFileSync } from "node:child_process"

const ALBUM = "Test Tones"
const ALBUM_ARTIST = "MediaFlick Fixtures"
const AUDIOBOOK = "Aesop's Fables"
const AUDIOBOOK_RESUME_S = 600

export default async function music(ctx) {
  await ctx.signIn({})
  const linux = process.platform === "linux"
  const builtIn = ctx.readConfig("settings.json")?.player_backend !== "mpv"
  const sink = linux ? await ctx.audioSink() : null
  const report = { backend: builtIn ? "builtin" : "external", checks: [], tracks: [], eof: [], session: [], keys: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` ${JSON.stringify(detail)}`}`)
  }
  const audio = async () => {
    const player = await ctx.player()
    return player.active && player.nowPlaying?.kind === "audio" ? { ...player.nowPlaying, positionMs: player.positionMs, paused: player.paused } : null
  }
  const playingIndex = (index, description, timeout = 30000) =>
    ctx.until(async () => { const now = await audio(); return now?.index === index && now.phase === "playing" }, description, timeout)
  const status = (await ctx.api("/api/status")).body
  const session = async () => (await ctx.jellyfin(`/Sessions?deviceId=${encodeURIComponent(status.deviceId)}`)).find((entry) => entry.NowPlayingItem)
  const reports = () => ctx.appLog().split("\n")
    .filter((line) => line.includes(": queued Jellyfin playback state"))
    .map((line) => ({ endpoint: line.match(/endpoint="([^"]+)"/)?.[1], body: JSON.parse(line.slice(line.indexOf("body=") + 5)) }))
  const capture = async (name) => {
    if (linux && builtIn) ctx.displayCapture(name)
    await ctx.screenshot(name)
  }
  const pressKey = (keysym) => execFileSync("xdotool", ["key", keysym])

  const albums = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&IncludeItemTypes=MusicAlbum&Recursive=true&SearchTerm=${encodeURIComponent(ALBUM)}`)
  const album = albums.Items.find((item) => item.Name === ALBUM && item.AlbumArtist === ALBUM_ARTIST)
  if (!album) throw new Error(`the server has no album "${ALBUM}" by ${ALBUM_ARTIST}; add the fixture first`)
  const oracle = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&ParentId=${album.Id}&IncludeItemTypes=Audio&Recursive=true&SortBy=ParentIndexNumber,IndexNumber,SortName`)).Items
  report.album = { id: album.Id, tracks: oracle.map((track) => ({ id: track.Id, name: track.Name, disc: track.ParentIndexNumber, track: track.IndexNumber, container: track.Container })) }
  const ids = oracle.map((track) => track.Id)
  for (const id of ids) ctx.restoreItemAfterRun(id)
  if (ids.length !== 5) throw new Error(`"${ALBUM}" should have 5 tracks, has ${ids.length}`)

  // Music always starts at the beginning, whatever the server stored.
  await ctx.setResumeOnServer(ids[0], 70_000_000)

  await ctx.press({ role: "link", name: "Music" })
  await ctx.press({ role: "link", name: `Open details for ${ALBUM}` }, { nth: 0 })
  await ctx.find({ role: "button", name: "Shuffle" }, { timeout: 30000 })
  const rowOrder = await ctx.app.evaluate(`[...document.querySelectorAll("section[aria-label=Tracks] button[aria-label^='Play ']")].map((row) => row.getAttribute("aria-label").slice(5))`)
  check(rowOrder.join() === oracle.map((track) => track.Name).join(), "the album page lists its tracks in disc and track order", rowOrder)
  check(await ctx.exists({ role: "heading", name: "Disc 2" }), "the album page heads its second disc")
  await ctx.snapshot("01-album")
  await capture("01-album")

  await ctx.press({ role: "button", name: "Play" })
  await playingIndex(0, "the album's first track to play")
  const first = await audio()
  report.firstStartMs = first.positionMs
  check(first.length === 5 && first.current.itemId === ids[0] && first.next?.itemId === ids[1], "Play queued the album from its first track", { length: first.length, current: first.current.title, next: first.next?.title })
  check(first.positionMs < 3000, "the first track started at 0 despite a stored resume point", first.positionMs)
  check(!(await ctx.app.evaluate(`document.documentElement.hasAttribute("data-libmpv-playback")`)), "audio leaves the page opaque: no player takeover")
  report.mpv = { vid: await ctx.mpv("vid"), audioDisplay: await ctx.mpv("audio-display") }
  check(report.mpv.vid === false && report.mpv.audioDisplay === false, "mpv plays the track with no picture", report.mpv)
  if (!builtIn) {
    const mpvWindows = (() => { try { return execFileSync("xdotool", ["search", "--class", "mpv"], { encoding: "utf8" }).trim() } catch { return "" } })()
    check(mpvWindows === "", "External mpv opened no window for audio", mpvWindows)
  }
  if (sink) {
    report.level = await sink.capture("02-track1", 1.5)
    check(report.level.meanDb > -60, "sound reached the audio device", report.level)
  }
  await ctx.find({ role: "button", name: "Next track" })
  await capture("02-playing-album")

  // Natural EOF through all five tracks, crossing from disc 1 to disc 2.
  for (let index = 0; index < ids.length; index++) {
    await playingIndex(index, `track ${index + 1} to play`, 40000)
    const now = await audio()
    report.tracks.push({ index, itemId: now.current.itemId, title: now.current.title, disc: now.current.discNumber, playlistItemId: now.playlistItemId })
    await ctx.until(async () => (await session())?.PlaylistItemId === now.playlistItemId, `Jellyfin's session to show track ${index + 1}`, 15000)
    const live = await session()
    report.session.push({ index, item: live.NowPlayingItem.Id, playlistItemId: live.PlaylistItemId, queue: live.NowPlayingQueue?.map((entry) => entry.Id) })
    if (index === 1) await capture("03-track2")
    if (index === 0) {
      await ctx.app.evaluate("document.activeElement?.blur()")
      await ctx.key(" ", { code: "Space", keyCode: 32 })
      const paused = await ctx.until(async () => (await audio())?.paused === true, "Space on the page to pause", 5000).then(() => true, () => false)
      await ctx.key(" ", { code: "Space", keyCode: 32 })
      const resumed = await ctx.until(async () => (await audio())?.paused === false, "Space again to resume", 5000).then(() => true, () => false)
      check(paused && resumed, "Space pauses and resumes audio while the page has focus")
    }
    // Track 1 plays to its end untouched; ArrowRight (+30 s) takes the others there.
    if (index > 0) {
      await ctx.app.evaluate("document.activeElement?.blur()")
      await ctx.key("ArrowRight", { code: "ArrowRight", keyCode: 39 })
    }
    const started = Date.now()
    if (index < ids.length - 1) {
      await playingIndex(index + 1, `EOF of track ${index + 1} to advance`, 20000)
      report.eof.push({ from: index, ms: Date.now() - started })
    } else {
      await ctx.until(async () => (await ctx.player()).active === false, "the queue to end after its last track", 20000)
      report.eof.push({ from: index, ms: Date.now() - started, ended: true })
    }
  }
  check(report.tracks.map((track) => track.itemId).join() === ids.join(), "EOF advanced through all five tracks in order without input", report.tracks.map((track) => track.title))
  check(report.tracks[2].disc === 1 && report.tracks[3].disc === 2, "the queue crossed from disc 1 to disc 2")
  check(new Set(report.tracks.map((track) => track.playlistItemId)).size === 5, "each track has its own PlaylistItemId")
  check(report.session.every((entry, index) => entry.item === ids[index] && entry.playlistItemId === report.tracks[index].playlistItemId), "Jellyfin's session showed each track with its PlaylistItemId", report.session.map((entry) => entry.playlistItemId))
  check(report.session.slice(1).every((entry) => entry.queue?.join() === ids.join()), "Jellyfin's session held the album's NowPlayingQueue from the second track on (it records the queue from a Stopped report)")
  const bodies = reports()
  const byEndpoint = (endpoint) => bodies.filter((entry) => entry.endpoint === endpoint)
  report.reports = { playing: byEndpoint("Sessions/Playing").length, progress: byEndpoint("Sessions/Playing/Progress").length, stopped: byEndpoint("Sessions/Playing/Stopped").length }
  check(ids.every((id) => ["Sessions/Playing", "Sessions/Playing/Stopped"].every((endpoint) => byEndpoint(endpoint).some((entry) => entry.body.ItemId === id))), "every track got a Playing and a Stopped report", report.reports)
  check(bodies.every(({ body }) => body.NowPlayingQueue?.map((entry) => entry.Id).join() === ids.join() && body.PlaylistItemId), "every report carried NowPlayingQueue and PlaylistItemId", bodies.length)
  check(!(await ctx.exists({ role: "button", name: "Next track" })), "the bar left once the queue ended")

  // Play from a track on disc 2.
  await ctx.press({ role: "button", name: `Play ${oracle[3].Name}` })
  await playingIndex(3, "the queue to start at track 4")
  const fromTrack = await audio()
  check(fromTrack.current.itemId === ids[3] && fromTrack.canPrevious && fromTrack.next?.itemId === ids[4], "choosing a track plays the album from it, earlier tracks still reachable")

  // Bar Next and Previous, then the media keys.
  await ctx.press({ role: "button", name: "Previous track" })
  await playingIndex(2, "Previous from the bar to go back to track 3")
  await ctx.press({ role: "button", name: "Next track" })
  await playingIndex(3, "Next from the bar to reach track 4")
  await ctx.until(async () => (await audio())?.positionMs > 4000, "track 4 past 4 s")
  await ctx.press({ role: "button", name: "Previous track" })
  await ctx.until(async () => { const now = await audio(); return now?.index === 3 && now.positionMs < 2500 }, "Previous after 3 s to restart track 4")
  check(true, "the bar's Previous restarts after 3 s and goes back within 3 s; Next moves on")

  if (linux) {
    const window = execFileSync("xdotool", ["search", "--onlyvisible", "--class", "io.github.phob.MediaFlickDesktop"], { encoding: "utf8" }).trim().split("\n")[0]
    execFileSync("xdotool", ["windowfocus", "--sync", window])
    await ctx.app.evaluate(`document.activeElement?.blur()`)
    const key = async (keysym, condition, description) => {
      pressKey(keysym)
      const ok = await ctx.until(condition, description, 10000).then(() => true, () => false)
      report.keys.push({ keysym, ok })
      check(ok, description)
    }
    await key("XF86AudioPrev", async () => (await audio())?.index === 2, "XF86AudioPrev went back to track 3")
    await key("XF86AudioNext", async () => (await audio())?.index === 3, "XF86AudioNext moved on to track 4")
    await key("XF86AudioPlay", async () => (await audio())?.paused === true, "XF86AudioPlay paused")
    await key("XF86AudioPlay", async () => (await audio())?.paused === false, "XF86AudioPlay resumed")
  }

  // Browse while it plays.
  const before = (await audio()).positionMs
  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library/"), "the Movies library while music plays")
  await capture("04-browsing-movies")
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/settings"), "Settings while music plays")
  await capture("05-browsing-settings")
  const during = await audio()
  check(during?.index === 3 && during.positionMs > before, "playback ran on while browsing other pages", { before, after: during?.positionMs })
  check(await ctx.exists({ role: "button", name: "Next track" }), "the now-playing bar stayed on other pages")

  // Shuffle replaces the queue with an order of the whole album.
  await ctx.press({ role: "link", name: "Music" })
  await ctx.press({ role: "link", name: `Open details for ${ALBUM}` }, { nth: 0 })
  await ctx.press({ role: "button", name: "Shuffle" })
  await ctx.until(async () => { const now = await audio(); return now?.phase === "playing" && now.index === 0 && !report.tracks.some((track) => track.playlistItemId === now.playlistItemId) }, "the shuffled queue to start")
  await ctx.until(() => reports().some(({ endpoint, body }) => endpoint === "Sessions/Playing" && !body.PlaylistItemId.startsWith(report.tracks[0].playlistItemId.split("-")[0] + "-") && body.NowPlayingQueue.length === 5), "a Playing report for the shuffled queue")
  const shuffled = reports().filter(({ endpoint }) => endpoint === "Sessions/Playing").at(-1).body.NowPlayingQueue.map((entry) => entry.Id)
  report.shuffle = shuffled
  check(shuffled.length === 5 && [...shuffled].sort().join() === [...ids].sort().join(), "Shuffle queued every track of the album once", shuffled)

  await ctx.press({ role: "button", name: "Stop" })
  await ctx.until(async () => (await ctx.player()).active === false, "Stop to end the queue")
  check(await ctx.mpv("idle-active") === true, "Stop left mpv idle")
  check(String(await ctx.mpv("vid")) !== "false", "mpv selects video again for the next file after audio", await ctx.mpv("vid"))

  // "Play On" from another client: an item list in the sender's order, and an album.
  const target = await ctx.remoteCommand("Playing", { playCommand: "PlayNow", itemIds: [ids[1], ids[4]].join(","), startIndex: 1 })
  report.remote = { playableMediaTypes: target.PlayableMediaTypes, supportsMediaControl: target.SupportsMediaControl }
  check(target.PlayableMediaTypes?.includes("Audio") && target.PlayableMediaTypes.includes("Video"), "the app's session advertises Audio and Video", target.PlayableMediaTypes)
  await ctx.until(async () => { const now = await audio(); return now?.phase === "playing" && now.length === 2 && now.index === 1 }, "the remote two-item queue to play at StartIndex 1")
  check((await audio()).current.itemId === ids[4], "remote PlayNow played ItemIds[StartIndex] of an out-of-order list")
  await ctx.remoteCommand("Playing/PreviousTrack")
  await ctx.until(async () => { const now = await audio(); return now?.index === 0 && now.current.itemId === ids[1] && now.phase === "playing" }, "remote PreviousTrack to reach the list's first item")
  await ctx.remoteCommand("Playing/NextTrack")
  await ctx.until(async () => { const now = await audio(); return now?.index === 1 && now.phase === "playing" }, "remote NextTrack to move on")
  check(true, "remote PreviousTrack and NextTrack moved through the remote queue")
  await ctx.remoteCommand("Playing", { playCommand: "PlayNow", itemIds: album.Id })
  await ctx.until(async () => { const now = await audio(); return now?.phase === "playing" && now.length === 5 && now.current.itemId === ids[0] }, "a remote album to play as its five tracks")
  check(true, "remote PlayNow of an album queued its tracks in order")
  if (!builtIn) {
    const bindings = await ctx.mpv("input-bindings")
    const winner = (key) => bindings.filter((binding) => binding.key === key).sort((a, b) => b.priority - a.priority)[0]?.cmd
    report.mpvBindings = { NEXT: winner("NEXT"), PREV: winner("PREV") }
    check(report.mpvBindings.NEXT === "script-message mediaflick-desktop next-track" && report.mpvBindings.PREV === "script-message mediaflick-desktop previous-track", "External mpv's own Next and Previous keys go to the app's queue", report.mpvBindings)
  }
  await ctx.remoteCommand("Playing/Stop")
  await ctx.until(async () => (await ctx.player()).active === false, "remote Stop to end the queue")

  // An audiobook continues from its saved position.
  const books = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&IncludeItemTypes=AudioBook&Recursive=true&SearchTerm=${encodeURIComponent(AUDIOBOOK)}`)
  const book = books.Items.find((item) => item.Name === AUDIOBOOK)
  if (!book) throw new Error(`the server has no audiobook "${AUDIOBOOK}"`)
  ctx.restoreItemAfterRun(book.Id)
  await ctx.setResumeOnServer(book.Id, AUDIOBOOK_RESUME_S * 10_000_000)
  await ctx.press({ role: "link", name: "Books" })
  await ctx.press({ role: "link", name: `Open details for ${AUDIOBOOK}` }, { nth: 0 })
  const resumeLabel = `Continue at ${Math.floor(AUDIOBOOK_RESUME_S / 60)}:${String(AUDIOBOOK_RESUME_S % 60).padStart(2, "0")}`
  await ctx.find({ role: "button", name: resumeLabel }, { timeout: 30000 })
  check(await ctx.exists({ role: "button", name: "From start" }), "the audiobook offers Continue and From start")
  await capture("06-audiobook")
  await ctx.press({ role: "button", name: resumeLabel })
  // mpv needs about 20 s to open this .m4b: ffmpeg reads its 285 chapter
  // samples from all over the 93 MB file before it reports the file loaded.
  await ctx.until(async () => { const now = await audio(); return now?.mediaKind === "audiobook" && now.phase === "playing" }, "the audiobook to play", 60000)
  await ctx.until(async () => Number(await ctx.mpv("time-pos")) > AUDIOBOOK_RESUME_S - 15, "mpv to reach the saved position", 20000)
  const bookPosition = Number(await ctx.mpv("time-pos"))
  report.audiobook = { id: book.Id, resumeS: AUDIOBOOK_RESUME_S, mpvS: bookPosition, chapters: (await ctx.player()).chapters.length }
  check(Math.abs(bookPosition - AUDIOBOOK_RESUME_S) < 15, "the audiobook continued from its saved position", report.audiobook)
  check(/sending delayed mpv startup seek/.test(ctx.appLog()), "the resume used the delayed startup seek")
  check(report.audiobook.chapters > 0, "the bar has the audiobook's chapters")
  await capture("07-audiobook-playing")
  await ctx.press({ role: "button", name: "Stop" })
  await ctx.until(async () => (await ctx.player()).active === false, "Stop to end the audiobook")
  await ctx.until(async () => {
    const data = (await ctx.jellyfin(`/Items/${book.Id}?userId=${ctx.account.userId}`)).UserData
    return Math.abs(data.PlaybackPositionTicks / 1e7 - bookPosition) < 30
  }, "Jellyfin to store the audiobook's new position", 20000)
  check(true, "Jellyfin stored the audiobook position at Stop")

  report.playstates = ctx.playstates()
  ctx.writeEvidence("music.json", JSON.stringify(report, null, 2))
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} music checks failed: ${failed.map((entry) => entry.message).join("; ")}`)
}
