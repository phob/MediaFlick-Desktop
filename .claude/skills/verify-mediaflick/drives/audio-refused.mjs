// A load the player refuses ends the audio queue. With External mpv pointed
// at an executable that does not exist, Play on the `Test Tones` album must
// reach the UI as one playback failure and then one Stopped, the queue's
// terminal event, describing the queue with an error; the bar must be gone
// within 3 s and the player idle. Writes audio-refused.json and a screenshot.
//   just verify audio-refused --account SERVER --seed .claude/skills/verify-mediaflick/seeds/missing-mpv
const ALBUM = "Test Tones"
const ALBUM_ARTIST = "MediaFlick Fixtures"
const MISSING_MPV = "/nonexistent/mpv"
const FAILED = "player backend reported a playback failure"
const BAR_LEAVES_WITHIN_MS = 3000
const WATCH_MS = 15000

function dispatchedStops(log) {
  return log.split("\n")
    .filter((line) => line.includes("dispatched playback event to WebUI") && line.includes("event=Stopped("))
    .map((line) => ({
      at: line.slice(0, line.indexOf(" ")),
      kind: line.match(/now_playing: Some\((Video|Audio)/)?.[1] ?? "none",
      reason: line.match(/stop_reason: (?:Some\((\w+)\)|None) \}/)?.[1] ?? null,
    }))
}

export default async function audioRefused(ctx) {
  await ctx.signIn({})
  const settings = ctx.readConfig("settings.json")
  if (settings?.player_backend !== "mpv" || settings.mpv_path !== MISSING_MPV) {
    throw new Error("seed External mpv at a missing executable: --seed .claude/skills/verify-mediaflick/seeds/missing-mpv")
  }
  const report = { checks: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` ${JSON.stringify(detail)}`}`)
  }
  const album = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&IncludeItemTypes=MusicAlbum&Recursive=true&SearchTerm=${encodeURIComponent(ALBUM)}`))
    .Items.find((item) => item.Name === ALBUM && item.AlbumArtist === ALBUM_ARTIST)
  if (!album) throw new Error(`the server has no album "${ALBUM}" by ${ALBUM_ARTIST}; add the fixture first`)
  for (const track of (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&ParentId=${album.Id}&IncludeItemTypes=Audio&Recursive=true`)).Items) ctx.restoreItemAfterRun(track.Id)

  await ctx.press({ role: "link", name: "Music" })
  await ctx.press({ role: "link", name: `Open details for ${ALBUM}` }, { nth: 0 })
  await ctx.press({ role: "button", name: "Play" })
  await ctx.until(() => ctx.appLog().includes(FAILED), "the player to refuse the album's first track", 30000)
  const failedAt = Date.now()
  const sinceFailure = () => { const log = ctx.appLog(); return log.slice(log.indexOf(FAILED)) }

  let barLeftAfterMs = null
  await ctx.until(async () => {
    if (!(await ctx.exists({ role: "button", name: "Next track" }))) barLeftAfterMs = Date.now() - failedAt
    return barLeftAfterMs !== null || Date.now() - failedAt > WATCH_MS
  }, `the bar to leave, or ${WATCH_MS / 1000} s`, WATCH_MS + 5000)
  await ctx.screenshot("01-after-refusal")
  const player = await ctx.player()
  const stops = dispatchedStops(sinceFailure())
  const failures = sinceFailure().split(FAILED).length - 1
  Object.assign(report, { barLeftAfterMs, stops, failures, player: { active: player.active, nowPlaying: player.nowPlaying ?? null } })

  check(failures === 1, "the refusal reached the UI as one playback failure", failures)
  check(stops.length === 1 && stops[0].kind === "Audio" && stops[0].reason === "Error", "then one Stopped ended the audio queue with an error", stops)
  check(barLeftAfterMs !== null && barLeftAfterMs < BAR_LEAVES_WITHIN_MS, `the bar left within ${BAR_LEAVES_WITHIN_MS / 1000} s of the failure`, barLeftAfterMs)
  check(player.active === false, "the player reports nothing playing", report.player)

  ctx.writeEvidence("audio-refused.json", JSON.stringify(report, null, 2))
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} audio-refused checks failed: ${failed.map((entry) => entry.message).join("; ")}`)
}
