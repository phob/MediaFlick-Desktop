import { listProcesses } from "../scripts/platform.mjs"

function findHelperProcesses() {
  return listProcesses().filter(
    (row) => row.cmd.includes("--player-helper") && row.cmd.includes("mediaflick-desktop"),
  )
}

async function selectRecoverableMovie(ctx) {
  const { Items } = await ctx.jellyfin(
    `/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&isPlayed=false&fields=MediaStreams&limit=250`,
  )
  const movie = Items.find((item) => {
    const stream = item.MediaStreams?.find((entry) => entry.Type === "Video")
    return !item.UserData?.Played && !item.UserData?.PlaybackPositionTicks && stream?.Codec === "h264" && stream.Height <= 1080 && item.RunTimeTicks > 10 * 60 * 1e7
  })
  if (!movie) throw new Error("helper-recovery requires an unplayed H.264 movie longer than 10 minutes without resume position")
  return movie
}

async function startOrReplayMovie(ctx, movieId) {
  const fromStart = { role: "button", name: "From start" }
  const playBtn = { role: "button", name: "Play" }
  const targetBtn = await ctx.until(async () => {
    if (await ctx.exists(fromStart)) return fromStart
    if (await ctx.exists(playBtn)) return playBtn
    return null
  }, "Play or From start button on movie detail page", 20000)
  await ctx.press(targetBtn)
  await ctx.until(async () => {
    const player = await ctx.player()
    return player?.active && player.itemId === movieId && player.positionMs > 1500
  }, "built-in movie playback advancing", 90000)
  if ((await ctx.mpv("mute")) !== true) {
    await ctx.key("m", { code: "KeyM", keyCode: 77 })
    await ctx.until(async () => (await ctx.mpv("mute")) === true, "mpv muted", 10000)
  }
}

export default async function helperRecovery(ctx) {
  if (process.platform !== "linux") throw new Error("helper-recovery is a Linux out-of-process helper drive")
  await ctx.audioSink()
  await ctx.signIn({})

  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "initial sync completion", 300000)

  const settings = (await ctx.api("/api/settings")).body
  ctx.check(settings?.client?.player?.playerBackend === "libmpv", "playerBackend is built-in libmpv")
  await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "built-in mpv helper warm and idle", 30000)

  const movie = await selectRecoverableMovie(ctx)
  ctx.restoreItemAfterRun(movie.Id)

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, movie.Name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "library search route", 20000)
  await ctx.press({ role: "link", name: `Open details for ${movie.Name}` }, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${movie.Id}`, "movie detail page loaded", 20000)

  await startOrReplayMovie(ctx, movie.Id)

  const initialHelpers = findHelperProcesses()
  ctx.check(initialHelpers.length >= 1, `found running --player-helper process (${initialHelpers.length})`)
  const initialHelperPid = initialHelpers[0]?.pid
  if (!Number.isInteger(initialHelperPid) || initialHelperPid <= 0) {
    throw new Error(`invalid initial helper PID: ${JSON.stringify(initialHelpers)}`)
  }

  process.kill(initialHelperPid, "SIGKILL")

  const statusAfterKill = await ctx.until(async () => {
    const res = await ctx.api("/api/status")
    return res.status === 200 ? res.status : null
  }, "main desktop process stayed responsive after helper SIGKILL", 15000)

  await ctx.until(async () => !(await ctx.player())?.active, "crashed playback session cleared cleanly", 25000)

  await ctx.until(
    () => findHelperProcesses().some((row) => row.pid !== initialHelperPid),
    "new --player-helper process spawned after crash",
    20000,
  )

  await startOrReplayMovie(ctx, movie.Id)
  const respawnedHelpers = findHelperProcesses().filter((row) => row.pid !== initialHelperPid)
  const respawnedHelperPid = respawnedHelpers[0]?.pid ?? null
  ctx.check(Number.isInteger(respawnedHelperPid) && respawnedHelperPid !== initialHelperPid, `playback recovered on new helper PID ${respawnedHelperPid}`)

  const replayedPositionMs = (await ctx.player())?.positionMs ?? 0
  ctx.displayCapture("helper-recovered-display")

  await ctx.key("q", { code: "KeyQ", keyCode: 81 })
  await ctx.until(async () => !(await ctx.player())?.active, "recovered playback stopped cleanly", 30000)
  await ctx.screenshot("helper-recovered")

  const report = {
    movieId: movie.Id,
    movieName: movie.Name,
    initialHelperPid,
    respawnedHelperPid,
    statusAfterKill,
    replayedPositionMs,
  }
  ctx.writeEvidence("helper-recovery-report.json", report)
  ctx.step(`helper recovery verified: pid ${initialHelperPid} -> SIGKILL -> pid ${respawnedHelperPid}`)
}
