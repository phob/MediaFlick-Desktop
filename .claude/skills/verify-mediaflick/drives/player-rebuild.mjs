// The player rebuilt live under a playing audio queue: with External mpv, a
// changed executable path replaces the adapter on Save. The retired adapter's
// own events (its shutdown's Stopped) must never reach the UI, so the rebuild
// ends the queue with exactly one Stopped, and the next queue on the new
// player plays undisturbed. Writes the test user's playstate and puts it back.
//   just verify player-rebuild --account SERVER --seed .claude/skills/verify-mediaflick/seeds/external-mpv
import { realpathSync } from "node:fs"

const ALBUM = "Test Tones"
const ALBUM_ARTIST = "MediaFlick Fixtures"
const REBUILT = "rebuilding player backend after settings change"
const RETIRED = "received playback shutdown request"
const MPV = "/usr/bin/mpv"
const SAME_MPV = "/bin/mpv"

function dispatchedStops(log) {
  return log.split("\n")
    .filter((line) => line.includes("dispatched playback event to WebUI") && line.includes("event=Stopped("))
    .map((line) => ({
      at: line.slice(0, line.indexOf(" ")),
      kind: line.match(/now_playing: Some\((Video|Audio)/)?.[1] ?? "none",
      playbackId: line.match(/active: (?:true|false), playback_id: (?:Some\((-?\d+)\)|None)/)?.[1] ?? null,
      reason: line.match(/stop_reason: (?:Some\((\w+)\)|None) \}/)?.[1] ?? null,
    }))
}

export default async function playerRebuild(ctx) {
  await ctx.signIn({})
  if (ctx.readConfig("settings.json")?.player_backend !== "mpv") throw new Error("seed External mpv: --seed .claude/skills/verify-mediaflick/seeds/external-mpv")
  if (realpathSync(SAME_MPV) !== realpathSync(MPV)) throw new Error(`${SAME_MPV} must be the same file as ${MPV}`)
  if (process.platform === "linux") await ctx.audioSink()
  const report = { checks: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` ${JSON.stringify(detail)}`}`)
  }
  const audio = async () => {
    const player = await ctx.player()
    return player.active && player.nowPlaying?.kind === "audio" ? { playbackId: player.playbackId, index: player.nowPlaying.index, phase: player.nowPlaying.phase, positionMs: player.positionMs } : null
  }

  const album = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&IncludeItemTypes=MusicAlbum&Recursive=true&SearchTerm=${encodeURIComponent(ALBUM)}`))
    .Items.find((item) => item.Name === ALBUM && item.AlbumArtist === ALBUM_ARTIST)
  if (!album) throw new Error(`the server has no album "${ALBUM}" by ${ALBUM_ARTIST}; add the fixture first`)
  for (const track of (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&ParentId=${album.Id}&IncludeItemTypes=Audio&Recursive=true`)).Items) ctx.restoreItemAfterRun(track.Id)
  const playAlbum = async (description) => {
    await ctx.press({ role: "link", name: "Music" })
    await ctx.press({ role: "link", name: `Open details for ${ALBUM}` }, { nth: 0 })
    await ctx.press({ role: "button", name: "Play" })
    await ctx.until(async () => (await audio())?.phase === "playing", description, 30000)
    return audio()
  }

  const before = await playAlbum("the album to play on the first player")
  report.first = before
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 15000 })
  await ctx.fill({ role: "textbox", name: "mpv executable" }, SAME_MPV)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.mpv_path === SAME_MPV, "settings.json to record the new mpv path")
  await ctx.until(() => ctx.appLog().includes(REBUILT), "the app to rebuild the player")
  await ctx.until(() => ctx.appLog().slice(ctx.appLog().indexOf(REBUILT)).includes(RETIRED), "the retired player to shut down")
  await ctx.until(async () => (await ctx.player()).active === false, "the rebuild to end the queue")
  const settled = Date.now() + 3000
  await ctx.until(() => Date.now() > settled, "3 s for any late event of the retired player")
  await ctx.screenshot("01-after-rebuild")

  const sinceRebuild = () => { const log = ctx.appLog(); return log.slice(log.indexOf(REBUILT)) }
  const rebuildStops = dispatchedStops(sinceRebuild())
  report.rebuildStops = rebuildStops
  check(rebuildStops.length === 1, "the rebuild sent the UI exactly one Stopped", rebuildStops)
  check(rebuildStops.every((stop) => stop.kind === "Audio"), "every Stopped of the rebuild describes the audio queue it ended", rebuildStops.map((stop) => stop.kind))

  const after = await playAlbum("the album to play on the rebuilt player")
  report.second = after
  check(after.playbackId !== before.playbackId, "the new queue plays a new load", { before: before.playbackId, after: after.playbackId })
  const startedAt = after.positionMs
  await ctx.until(async () => { const now = await audio(); return now && now.positionMs > startedAt + 3000 }, "the new queue to play on for 3 s", 15000)
  const lateStops = dispatchedStops(sinceRebuild()).slice(rebuildStops.length)
  report.lateStops = lateStops
  check(lateStops.length === 0, "nothing stopped the new queue", lateStops)
  check(!sinceRebuild().includes("player backend reported a playback failure"), "no playback failure reached the UI after the rebuild")
  await ctx.screenshot("02-new-queue")
  await ctx.press({ role: "button", name: "Stop" })
  await ctx.until(async () => (await ctx.player()).active === false, "Stop to end the new queue")

  ctx.writeEvidence("player-rebuild.json", JSON.stringify(report, null, 2))
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} player-rebuild checks failed: ${failed.map((entry) => entry.message).join("; ")}`)
}
