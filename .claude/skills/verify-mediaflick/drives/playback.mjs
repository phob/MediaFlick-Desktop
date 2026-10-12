// Playback end to end on your own server, on whichever backend the profile
// has: play from the start, seek past Jellyfin's resume minimum, pause, stop
// from the player bar, the resume point Jellyfin stored, Resume with the
// startup seek rules from AGENTS.md, stop again, then watched and unwatched
// to leave the title as it was. Plays real audio.
//   just verify playback --account https://jellyfin.example.com --log-level "debug,jellyfin.playstate=trace,playback=trace"
// External mpv: add `--seed DIR` with a settings.json holding
//   {"player_backend":"mpv","mpv_path":"/usr/bin/mpv"}
// Writes the test user's playstate: it picks an unplayed movie without a
// resume point and leaves it unplayed without one.
const MIN_RESUME = 0.08 // Jellyfin stores no resume point below 5 % of the runtime
const status = async (ctx) => (await ctx.api("/api/status")).body
const ticksToS = (ticks) => ticks / 1e7

async function pickMovie(ctx) {
  const { Items } = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&isPlayed=false&sortBy=SortName&fields=MediaStreams&limit=200`)
  const movie = Items.find((item) => {
    const video = item.MediaStreams?.find((stream) => stream.Type === "Video")
    return !item.UserData?.PlaybackPositionTicks && item.RunTimeTicks > 60 * 60 * 1e7 && video?.Codec === "h264" && video.Height <= 1080
  })
  if (!movie) throw new Error("no unplayed H.264 movie of an hour or more without a resume point")
  return movie
}

async function userData(ctx, id) {
  return (await ctx.jellyfin(`/Items/${id}?userId=${ctx.account.userId}`)).UserData
}

const logSince = (ctx, marker) => ctx.appLog().slice(marker)

export default async function playback(ctx) {
  await ctx.signIn({})
  await ctx.until(async () => {
    const now = await status(ctx)
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  const movie = await pickMovie(ctx)
  const id = movie.Id
  ctx.restoreItemAfterRun(id)
  ctx.step(`movie: ${movie.Name} (${id}), ${Math.round(ticksToS(movie.RunTimeTicks) / 60)} min`)
  const { client, capabilities } = (await ctx.api("/api/settings")).body
  // Built-in on Linux draws the bar over the video and hides it; External mpv
  // plays in its own window and keeps the bar at the bottom of the page.
  const integrated = capabilities.integratedLibmpvOverlay === true
  const report = { backend: client.player.playerBackend, integrated, movie: { id, name: movie.Name, runtimeS: ticksToS(movie.RunTimeTicks) } }
  ctx.step(`backend ${report.backend}${integrated ? " (integrated overlay)" : ""}`)

  // Open it as a user: sidebar search, then the result card.
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.fill({ role: "textbox", name: "Search the library" }, movie.Name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${movie.Name}` }, { nth: 0 })
  await ctx.find({ role: "button", name: "Play" }, { timeout: 30000 })
  ctx.check(!(await ctx.exists({ role: "button", name: "Resume" })), "the detail page offers Play, not Resume")
  await ctx.snapshot("01-detail")

  // Play from the start.
  let marker = ctx.appLog().length
  await ctx.press({ role: "button", name: "Play" })
  const first = await ctx.until(async () => {
    const state = await ctx.player()
    return state?.active && state.itemId === id && state.positionMs > 2000 ? state : null
  }, "playback running", 90000)
  report.play = { playMethod: first.playMethod, durationMs: first.durationMs }
  ctx.check(/starting playback item_id=\S+ play_method=\S+ .*start=none/.test(logSince(ctx, marker)), "Play starts at the beginning (start=none)")
  const t1 = await ctx.mpv("time-pos")
  await ctx.sleep(2000)
  const t2 = await ctx.mpv("time-pos")
  ctx.check(t2 > t1 + 1 && (await ctx.mpv("pause")) === false, `mpv plays (time-pos ${t1.toFixed(1)} → ${t2.toFixed(1)} s)`)
  report.mpv = { vo: await ctx.mpv("current-vo"), hwdec: await ctx.mpv("hwdec-current"), timePos: [t1, t2] }
  ctx.displayCapture("02-playing")

  // Six quick presses of the forward key (30 s by default; the app ignores
  // auto-repeat, so a held key seeks once). ArrowRight works on both
  // backends; ArrowUp and the stop key only on the Built-in player.
  const beforeRapid = (await ctx.player()).positionMs
  for (let press = 0; press < 6; press++) await ctx.key("ArrowRight", { code: "ArrowRight", keyCode: 39 })
  await ctx.sleep(3000)
  report.rapidSeek = { presses: 6, expectedS: 180, movedS: ((await ctx.player()).positionMs - beforeRapid) / 1000 }
  ctx.step(`six quick presses of ArrowRight moved ${report.rapidSeek.movedS.toFixed(1)} s (expected 180 s)`)

  // Seek one press at a time past the resume minimum, each time waiting, as a
  // user would, until the player bar shows the new time. The key handler
  // seeks from the bar's position, so a press before it catches up repeats the
  // previous target and is lost; those are counted, not hidden.
  const target = report.play.durationMs * MIN_RESUME
  const barPosition = () => ctx.app.evaluate(`Number(document.querySelector('[role="slider"][aria-label="Playback position"]')?.getAttribute("aria-valuenow") ?? NaN)`)
  let presses = 0
  let lost = 0
  while ((await ctx.player()).positionMs < target) {
    const before = (await ctx.player()).positionMs
    await ctx.key("ArrowRight", { code: "ArrowRight", keyCode: 39 })
    presses++
    try {
      await ctx.until(async () => (await ctx.player()).positionMs >= before + 25000 && (await barPosition()) >= before + 25000, "a 30-second seek on the bar", 5000, { quiet: true })
    } catch {
      lost++
    }
    if (presses > 80) throw new Error("seeking did not reach the resume minimum")
  }
  report.spacedSeek = { presses, lost }
  ctx.step(`${presses} spaced ArrowRight presses (${lost} without effect) reached ${Math.round((await ctx.player()).positionMs / 1000)} s`)
  ctx.check(Math.abs((await ctx.mpv("time-pos")) * 1000 - (await ctx.player()).positionMs) < 3000, "mpv's time-pos matches the app's position after seeking")

  // Pause and play again with the pause key: each reports to Jellyfin at once.
  await ctx.key("k", { code: "KeyK", keyCode: 75 })
  await ctx.until(async () => (await ctx.player()).paused === true && (await ctx.mpv("pause")) === true, "paused in the app and in mpv")
  await ctx.revealPlayer()
  await ctx.find({ role: "button", name: "Resume" })
  await ctx.snapshot("03-paused-bar")
  await ctx.screenshot("03-paused-bar")
  ctx.displayCapture("03-paused-display")
  await ctx.key("k", { code: "KeyK", keyCode: 75 })
  await ctx.until(async () => (await ctx.player()).paused === false, "playing again")
  const pausedReports = ctx.playstates().queued.filter((entry) => entry.endpoint === "Sessions/Playing/Progress" && entry.itemId === id)
  ctx.check(pausedReports.length >= 2, `progress reports were queued (${pausedReports.length})`)

  // Stop from the player bar's overflow menu (Stop itself shows from 1536 px).
  await ctx.until(async () => ctx.playstates().queued.some((entry) => entry.endpoint === "Sessions/Playing/Progress" && entry.positionS * 1000 >= target - 1000), "a progress report past the resume minimum", 20000)
  await ctx.revealPlayer()
  await ctx.press({ role: "button", name: "More playback controls" })
  await ctx.press({ role: "menuitem", name: "Stop playback" })
  await ctx.until(async () => !(await ctx.player())?.active, "playback to stop", 30000)
  const stopped = ctx.playstates().queued.filter((entry) => entry.endpoint === "Sessions/Playing/Stopped" && entry.itemId === id).at(-1)
  ctx.check(stopped && /reporting active playback stopped failed=false reason="stop"/.test(ctx.appLog()), `a Stopped report at ${stopped?.positionS.toFixed(1)} s`)
  await ctx.until(() => ctx.playstates().sent.some((entry) => entry.endpoint === "Sessions/Playing/Stopped" && entry.status === 204), "Jellyfin to accept the Stopped report (204)")
  const resumeAt = stopped.positionS
  const stored = await ctx.until(async () => {
    const data = await userData(ctx, id)
    return Math.abs(ticksToS(data.PlaybackPositionTicks) - resumeAt) < 2 ? data : null
  }, "Jellyfin to store the resume point", 20000)
  report.stop = { reportedS: resumeAt, storedS: ticksToS(stored.PlaybackPositionTicks), played: stored.Played }
  await ctx.find({ role: "button", name: "Resume" }, { timeout: 30000 })
  ctx.check(await ctx.exists({ role: "button", name: "From start" }), "the detail page now offers Resume and From start")
  await ctx.snapshot("04-resumable")

  // Resume: the startup seek after file-loaded, the held position, no start option.
  marker = ctx.appLog().length
  await ctx.press({ role: "button", name: "Resume" })
  await ctx.until(async () => (await ctx.player())?.active, "resumed playback", 90000)
  await ctx.until(() => /mpv startup seek reached resume range/.test(logSince(ctx, marker)), "the startup seek to reach the resume range", 30000)
  const resumed = logSince(ctx, marker)
  const launchStart = Number(resumed.match(/starting playback item_id=\S+ .*?start=([\d.]+)s/)?.[1])
  ctx.check(Math.abs(launchStart - resumeAt) < 1, `Resume launches at the stored point (${launchStart} s)`)
  const order = ["loaded Jellyfin stream in mpv", "activating pending playback", "sending delayed mpv startup seek", "mpv startup seek reached resume range"].map((text) => resumed.indexOf(text))
  ctx.check(order.every((index, n) => index >= 0 && (n === 0 || index > order[n - 1])), "load → file-loaded activation → delayed startup seek → resume range, in that order")
  const loadfile = resumed.split("\n").find((line) => /command=loadfile/.test(line)) ?? ""
  ctx.check(!/#t=|start=/.test(loadfile.replace(/^.*?url=/, "")), "loadfile carries no start option or #t= fragment")
  // The log names the property a set_property sets (`set_property pause=false`).
  const startupCommands = resumed.slice(0, order[3]).split("\n").flatMap((line) => line.match(/sending mpv command command=(\S+(?: [\w-]+=\S*)?)/)?.[1] ?? [])
  ctx.check(!startupCommands.some((command) => /^set_property pause=/.test(command)), `no pause command during startup (${[...new Set(startupCommands)].join(", ")})`)
  const resumedReports = ctx.playstates().queued.filter((entry) => entry.itemId === id && entry.at > (stopped.at ?? ""))
  const startReport = resumedReports.find((entry) => entry.endpoint === "Sessions/Playing")
  ctx.check(startReport && Math.abs(startReport.positionS - resumeAt) < 1, `the start report holds the resume point (${startReport?.positionS.toFixed(1)} s)`)
  const timePos = await ctx.until(async () => {
    const value = await ctx.mpv("time-pos")
    return value > resumeAt - 3 ? value : null
  }, "mpv at the resume point", 15000)
  ctx.displayCapture("05-resumed")
  await ctx.sleep(11000)
  const early = ctx.playstates().queued.filter((entry) => entry.itemId === id && entry.at > (stopped.at ?? "") && entry.positionS < resumeAt - 3)
  ctx.check(early.length === 0, "no report after Resume went below the resume point")
  report.resume = { launchStartS: launchStart, startReportS: startReport.positionS, mpvTimePosS: timePos, startupCommands: [...new Set(startupCommands)] }

  // Stop again: the stop key with the Built-in player, the bar otherwise
  // (the app's stop key only applies to the integrated player).
  if (integrated) {
    await ctx.key("q", { code: "KeyQ", keyCode: 81 })
  } else {
    await ctx.press({ role: "button", name: "More playback controls" })
    await ctx.press({ role: "menuitem", name: "Stop playback" })
  }
  await ctx.until(async () => !(await ctx.player())?.active, "playback to stop again", 30000)
  ctx.check((ctx.appLog().match(/mpv IPC connected/g) ?? []).length === 1, "one mpv IPC connection for the whole run")

  // Leave the title unplayed without a resume point.
  await ctx.press({ role: "button", name: "Mark watched" })
  await ctx.find({ role: "button", name: "Mark unwatched" }, { timeout: 15000 })
  await ctx.until(async () => (await userData(ctx, id)).Played === true, "Jellyfin to mark it played")
  await ctx.press({ role: "button", name: "Mark unwatched" })
  await ctx.find({ role: "button", name: "Mark watched" }, { timeout: 15000 })
  const final = await ctx.until(async () => {
    const data = await userData(ctx, id)
    return data.Played === false ? data : null
  }, "Jellyfin to mark it unplayed")
  ctx.check(!final.PlaybackPositionTicks, "the title is unplayed with no resume point again")
  report.cleanup = { played: final.Played, positionTicks: final.PlaybackPositionTicks }
  report.playstates = ctx.playstates()
  ctx.writeEvidence("playback.json", report)
  await ctx.snapshot("06-restored")
}
