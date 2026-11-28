const SERVER_INPUT = { role: "textbox", name: "Server" }

async function pickTargetMovie(ctx) {
  const { Items } = await ctx.jellyfin(
    `/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&isPlayed=false&fields=MediaStreams&limit=250`,
  )
  const hit = Items.find((item) => {
    const stream = item.MediaStreams?.find((entry) => entry.Type === "Video")
    return !item.UserData?.Played && !item.UserData?.PlaybackPositionTicks && stream?.Codec === "h264" && stream.Height <= 1080 && item.RunTimeTicks > 10 * 60 * 1e7
  })
  if (!hit) throw new Error("playstate-outbox requires an unplayed H.264 movie longer than 10 minutes without resume position")
  return hit
}

export default async function playstateOutbox(ctx) {
  if (!ctx.account?.signedOut) throw new Error("playstate-outbox requires --account SERVER --signed-out")
  if (process.platform === "linux") await ctx.audioSink()

  let blockPlaystate = true
  const blocked = []
  const forwarded = []
  const answer = (url, method) => {
    if (method !== "POST" || !url.pathname.startsWith("/Sessions/Playing")) return null
    const record = { method, path: url.pathname, at: new Date().toISOString() }
    if (blockPlaystate) {
      blocked.push(record)
      return { status: 503, body: { Message: "Simulated outbox outage" } }
    }
    forwarded.push(record)
    return null
  }

  const proxy = await ctx.proxy({ upstream: ctx.account.server, answer })
  await ctx.fill(SERVER_INPUT, proxy.url)
  await ctx.quickConnect({ approve: true, timeout: 60000 })
  ctx.revokeAppSessionAfterRun()

  await ctx.until(async () => {
    const now = (await ctx.api("/api/status")).body
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "initial sync completion", 300000)

  const settings = (await ctx.api("/api/settings")).body
  ctx.check(settings?.capabilities?.integratedLibmpvOverlay === true, "built-in player overlay is active")
  await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "built-in mpv reaches idle-active", 30000)

  const movie = await pickTargetMovie(ctx)
  const targetS = Math.max(180, Math.round((movie.RunTimeTicks / 1e7) * 0.08))
  ctx.restoreItemAfterRun(movie.Id)

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, movie.Name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "library search navigation", 20000)
  await ctx.press({ role: "link", name: `Open details for ${movie.Name}` }, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${movie.Id}`, "movie detail page opened", 20000)

  await ctx.press({ role: "button", name: "Play" })
  await ctx.until(async () => {
    const state = await ctx.player()
    return state?.active && state.itemId === movie.Id && state.positionMs > 1500
  }, "movie playback started", 90000)

  if ((await ctx.mpv("mute")) !== true) {
    await ctx.key("m", { code: "KeyM", keyCode: 77 })
    await ctx.until(async () => (await ctx.mpv("mute")) === true, "mpv muted", 10000)
  }

  await ctx.mpvCommand(["seek", targetS, "absolute"])
  const reachedTargetMs = await ctx.until(async () => {
    const state = await ctx.player()
    return state?.active && state.positionMs >= (targetS - 10) * 1000 ? state.positionMs : null
  }, `playback position reached >= ${targetS - 10}s`, 30000)

  await ctx.key("q", { code: "KeyQ", keyCode: 81 })
  await ctx.until(async () => !(await ctx.player())?.active, "playback stopped during outage", 30000)

  await ctx.until(
    () =>
      blocked.length >= 1 &&
      ctx.playstates().queued.some((entry) => entry.endpoint === "Sessions/Playing/Stopped" && entry.itemId === movie.Id),
    "outbox queued Stopped during 503 outage",
    20000,
  )

  const preItem = await ctx.jellyfin(`/Items/${movie.Id}?userId=${ctx.account.userId}`)
  const preRelaunchPositionS = (preItem.UserData?.PlaybackPositionTicks ?? 0) / 1e7
  ctx.check(preRelaunchPositionS < targetS - 60, `Jellyfin position stayed uncommitted during outage (${preRelaunchPositionS.toFixed(1)}s)`)

  await ctx.relaunch()
  blockPlaystate = false

  await ctx.until(async () => {
    const now = (await ctx.api("/api/status")).body
    return now?.bootstrap?.state === "ready"
  }, "relaunched app bootstrap ready", 60000)

  await ctx.until(
    () => forwarded.some((entry) => entry.path === "/Sessions/Playing/Stopped"),
    "restarted app drained Stopped report from SQLite outbox",
    60000,
  )

  const postRelaunchPositionS = await ctx.until(async () => {
    const item = await ctx.jellyfin(`/Items/${movie.Id}?userId=${ctx.account.userId}`)
    const posS = (item.UserData?.PlaybackPositionTicks ?? 0) / 1e7
    return posS >= targetS - 15 && posS <= targetS + 30 ? posS : null
  }, `Jellyfin persisted ~${targetS}s resume point after relaunch outbox drain`, 45000)

  await ctx.screenshot("outbox-recovered")
  const report = {
    movieId: movie.Id,
    movieName: movie.Name,
    targetS,
    reachedTargetMs,
    blockedCount: blocked.length,
    blockedStoppedCount: blocked.filter((entry) => entry.path === "/Sessions/Playing/Stopped").length,
    preRelaunchPositionS,
    postRelaunchPositionS,
    forwardedAfterRelaunch: forwarded,
  }
  ctx.writeEvidence("outbox-report.json", report)
  ctx.step(`outbox verified: pre=${preRelaunchPositionS.toFixed(1)}s -> post=${postRelaunchPositionS.toFixed(1)}s across relaunch`)
}
