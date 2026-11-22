const MIN_RESUME = 0.08
const status = async (ctx) => (await ctx.api("/api/status")).body
const ticksToS = (ticks) => ticks / 1e7

function displayCapture(ctx, name) {
  if (process.platform === "linux") ctx.displayCapture(name)
  else ctx.step(`no display capture for ${name}: Linux only`)
}

async function stopFromBar(ctx) {
  const stop = { role: "button", name: "Stop" }
  const more = { role: "button", name: "More playback controls" }
  await ctx.revealPlayer()
  const direct = await ctx.until(async () => ((await ctx.exists(stop)) ? "direct" : (await ctx.exists(more)) ? "menu" : null), "the player bar's stop control", 15000)
  if (direct === "direct") return ctx.press(stop)
  await ctx.press(more)
  await ctx.press({ role: "menuitem", name: "Stop playback" })
}

const REVEAL_DEADZONE_PX = 10
const HIDE_DELAY_MS = 3000
const HOLD_MS = HIDE_DELAY_MS + 2000

const chrome = (ctx) => ctx.app.evaluate(`(() => {
  const [brand, bar] = document.querySelectorAll(".libmpv-overlay-chrome")
  return { brand: brand?.dataset.visible === "true", bar: bar?.dataset.visible === "true", cursorHidden: document.documentElement.hasAttribute("data-libmpv-cursor-hidden") }
})()`)

async function moveTo(ctx, [x, y]) {
  await ctx.app.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none" })
  const movedAt = Date.now()
  await ctx.sleep(60)
  return movedAt
}

async function glide(ctx, from, to, stepPx) {
  const distance = Math.hypot(to[0] - from[0], to[1] - from[1])
  const steps = Math.max(1, Math.ceil(distance / stepPx))
  let movedAt = 0
  for (let step = 1; step <= steps; step++) {
    movedAt = await moveTo(ctx, [from[0] + ((to[0] - from[0]) * step) / steps, from[1] + ((to[1] - from[1]) * step) / steps])
  }
  return movedAt
}

async function barShown(ctx, shown, description, { menuOpen = false } = {}) {
  const state = await chrome(ctx)
  const ok = state.bar === shown && state.brand === shown && state.cursorHidden === !shown
  const pause = menuOpen ? null : await ctx.exists({ role: "button", name: "Pause" })
  const reachable = pause === null ? "" : `, Pause ${pause ? "reachable" : "not reachable"}`
  ctx.check(ok && (pause === null || pause === shown), `${description} (bar ${state.bar ? "shown" : "hidden"}, brand ${state.brand ? "shown" : "hidden"}, cursor ${state.cursorHidden ? "hidden" : "shown"}${reachable})`)
  return state
}

async function waitForHide(ctx, since) {
  const elapsed = await ctx.until(async () => ((await chrome(ctx)).bar ? null : Date.now() - since), "the player bar to hide", HIDE_DELAY_MS + 5000, { quiet: true })
  return elapsed
}

async function checkBarReveal(ctx) {
  const [width, height] = await ctx.app.evaluate("[innerWidth, innerHeight]")
  const centre = [Math.round(width / 2), Math.round(height / 2)]
  const offset = ([x, y], [dx, dy]) => [x + dx, y + dy]
  const reveal = { deadzonePx: REVEAL_DEADZONE_PX, hideDelayMs: HIDE_DELAY_MS, window: [width, height] }

  ctx.step("bar reveal: glide to the middle of the video in 12 px steps, then rest")
  const rested = await glide(ctx, offset(centre, [-60, 0]), centre, 12)
  await barShown(ctx, true, "a move over the middle of the video shows the bar")
  await ctx.sleep(Math.max(0, rested + HIDE_DELAY_MS - 1000 - Date.now()))
  const before = await chrome(ctx)
  const sampledAfterMs = Date.now() - rested
  ctx.check(sampledAfterMs < HIDE_DELAY_MS - 200, `sampled the bar ${sampledAfterMs} ms after the last move, before the delay ends`)
  ctx.check(before.bar && before.brand && !before.cursorHidden, `the bar, brand and cursor are still shown ${sampledAfterMs} ms after the last move`)
  reveal.hiddenAfterMs = await waitForHide(ctx, rested)
  ctx.check(reveal.hiddenAfterMs >= HIDE_DELAY_MS - 100 && reveal.hiddenAfterMs <= HIDE_DELAY_MS + 700, `the bar hid ${reveal.hiddenAfterMs} ms after the last move (expected ${HIDE_DELAY_MS} ms)`)
  await barShown(ctx, false, "the bar, brand and cursor hide after the idle delay")
  await ctx.screenshot("02a-bar-hidden-idle")
  displayCapture(ctx, "02a-bar-hidden-idle-display")

  const jitter = [[3, 0], [0, -4], [-5, 5], [6, -6], [7, 7], [-9, 0], [0, 9], [0, 0]]
  ctx.step(`bar reveal: jitter within ${REVEAL_DEADZONE_PX} px of the rest point: ${jitter.map(([dx, dy]) => `(${dx},${dy})`).join(" ")}`)
  for (const delta of jitter) await moveTo(ctx, offset(centre, delta))
  await ctx.sleep(1000)
  await barShown(ctx, false, "moves within the deadzone leave the bar hidden")
  reveal.jitter = jitter.map(([dx, dy]) => Math.hypot(dx, dy))
  await ctx.screenshot("02b-bar-hidden-after-jitter")

  ctx.step("bar reveal: glide 32 px right across the middle of the video in 4 px steps")
  await glide(ctx, centre, offset(centre, [32, 0]), 4)
  await ctx.until(async () => (await chrome(ctx)).bar, "the player bar to show", 1000)
  await barShown(ctx, true, "a slow move past the deadzone over the middle of the video shows the bar")
  await ctx.screenshot("02c-bar-revealed")
  displayCapture(ctx, "02c-bar-revealed-display")

  await ctx.hover({ role: "button", name: "Pause" })
  await ctx.sleep(HOLD_MS)
  await barShown(ctx, true, `the bar stays while the pointer rests on it (${HOLD_MS} ms)`)

  const menu = { role: "menu", name: "Playback settings" }
  await ctx.press({ role: "button", name: "Playback settings" })
  await ctx.find(menu, { timeout: 5000 })
  const away = [Math.round(width * 0.3), Math.round(height * 0.35)]
  ctx.step(`bar reveal: pointer off the bar to (${away.join(",")}) with the menu open`)
  await moveTo(ctx, away)
  ctx.check(!(await ctx.app.evaluate(`document.querySelectorAll(".libmpv-overlay-chrome")[1]?.matches(":hover")`)), "the pointer left the bar")
  await ctx.sleep(HOLD_MS)
  reveal.focusInMenu = await ctx.app.evaluate(`({ role: document.activeElement?.getAttribute("role"), focusVisible: document.activeElement?.matches(":focus-visible"), inBarDom: document.querySelectorAll(".libmpv-overlay-chrome")[1]?.contains(document.activeElement) })`)
  ctx.step(`focus while the menu is open: ${JSON.stringify(reveal.focusInMenu)}`)
  ctx.check(await ctx.exists(menu), "the Playback settings menu is still open")
  await barShown(ctx, true, `the bar stays while one of its menus is open (${HOLD_MS} ms, pointer off the bar)`, { menuOpen: true })
  await ctx.screenshot("02d-bar-held-by-menu")
  displayCapture(ctx, "02d-bar-held-by-menu-display")

  await ctx.key("Escape", { keyCode: 27 })
  await ctx.until(async () => !(await ctx.exists(menu)), "the menu to close", 5000)
  reveal.focusAfterEscape = await ctx.app.evaluate(`({ label: document.activeElement?.getAttribute("aria-label"), focusVisible: document.activeElement?.matches(":focus-visible") })`)
  ctx.step(`focus after Escape: ${JSON.stringify(reveal.focusAfterEscape)}`)
  await ctx.sleep(HOLD_MS)
  await barShown(ctx, true, `the bar stays while keyboard focus is in it (${HOLD_MS} ms)`)

  ctx.step("bar reveal: click the middle of the video to take focus out of the bar")
  const clicked = await moveTo(ctx, centre)
  for (const type of ["mousePressed", "mouseReleased"]) await ctx.app.send("Input.dispatchMouseEvent", { type, x: centre[0], y: centre[1], button: "left", clickCount: 1 })
  ctx.check(!(await ctx.app.evaluate(`document.querySelectorAll(".libmpv-overlay-chrome")[1]?.contains(document.activeElement)`)), "focus left the bar")
  reveal.hiddenAfterReleaseMs = await waitForHide(ctx, clicked)
  ctx.check(reveal.hiddenAfterReleaseMs <= HIDE_DELAY_MS + 700, `with every hold released the bar hid ${reveal.hiddenAfterReleaseMs} ms after the last move`)
  ctx.check((await ctx.player()).paused === false, "the reveal checks left playback running")
  return reveal
}

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
  const integrated = capabilities.integratedLibmpvOverlay === true
  const report = { backend: client.player.playerBackend, integrated, movie: { id, name: movie.Name, runtimeS: ticksToS(movie.RunTimeTicks) } }
  ctx.step(`backend ${report.backend}${integrated ? " (integrated overlay)" : ""}`)

  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.fill({ role: "textbox", name: "Search the library" }, movie.Name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${movie.Name}` }, { nth: 0 })
  await ctx.find({ role: "button", name: "Play" }, { timeout: 30000 })
  ctx.check(!(await ctx.exists({ role: "button", name: "Resume" })), "the detail page offers Play, not Resume")
  await ctx.snapshot("01-detail")
  await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "mpv warm and idle before Play")

  let marker = ctx.appLog().length
  await ctx.press({ role: "button", name: "Play" })
  const first = await ctx.until(async () => {
    const state = await ctx.player()
    return state?.active && state.itemId === id && state.positionMs > 2000 ? state : null
  }, "playback running", 90000)
  report.play = { playMethod: first.playMethod, durationMs: first.durationMs }
  ctx.check(/starting playback item_id=\S+ play_method=\S+ .*start=none/.test(logSince(ctx, marker)), "Play starts at the beginning (start=none)")
  await ctx.key("m", { code: "KeyM", keyCode: 77 })
  await ctx.until(async () => (await ctx.mpv("mute")) === true, "mpv muted by the m key")
  const t1 = await ctx.mpv("time-pos")
  await ctx.sleep(2000)
  const t2 = await ctx.mpv("time-pos")
  ctx.check(t2 > t1 + 1 && (await ctx.mpv("pause")) === false, `mpv plays (time-pos ${t1.toFixed(1)} → ${t2.toFixed(1)} s)`)
  report.mpv = { vo: await ctx.mpv("current-vo"), hwdec: await ctx.mpv("hwdec-current"), timePos: [t1, t2] }
  displayCapture(ctx, "02-playing")
  if (integrated) report.reveal = await checkBarReveal(ctx)

  const beforeRapid = (await ctx.player()).positionMs
  for (let press = 0; press < 6; press++) await ctx.key("ArrowRight", { code: "ArrowRight", keyCode: 39 })
  await ctx.sleep(3000)
  report.rapidSeek = { presses: 6, expectedS: 180, movedS: ((await ctx.player()).positionMs - beforeRapid) / 1000 }
  ctx.step(`six quick presses of ArrowRight moved ${report.rapidSeek.movedS.toFixed(1)} s (expected 180 s)`)

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

  await ctx.key("k", { code: "KeyK", keyCode: 75 })
  await ctx.until(async () => (await ctx.player()).paused === true && (await ctx.mpv("pause")) === true, "paused in the app and in mpv")
  await ctx.revealPlayer()
  await ctx.find({ role: "button", name: "Resume" })
  await ctx.snapshot("03-paused-bar")
  await ctx.screenshot("03-paused-bar")
  displayCapture(ctx, "03-paused-display")
  await ctx.key("k", { code: "KeyK", keyCode: 75 })
  await ctx.until(async () => (await ctx.player()).paused === false, "playing again")
  const pausedReports = ctx.playstates().queued.filter((entry) => entry.endpoint === "Sessions/Playing/Progress" && entry.itemId === id)
  ctx.check(pausedReports.length >= 2, `progress reports were queued (${pausedReports.length})`)

  await ctx.until(async () => ctx.playstates().queued.some((entry) => entry.endpoint === "Sessions/Playing/Progress" && entry.positionS * 1000 >= target - 1000), "a progress report past the resume minimum", 20000)
  await stopFromBar(ctx)
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
  const startupCommands = resumed.slice(0, order[3]).split("\n").flatMap((line) => line.match(/sending mpv command command=(\S+(?: [\w-]+=\S*)?)/)?.[1] ?? [])
  ctx.check(!startupCommands.some((command) => /^set_property pause=/.test(command)), `no pause command during startup (${[...new Set(startupCommands)].join(", ")})`)
  const resumedReports = ctx.playstates().queued.filter((entry) => entry.itemId === id && entry.at > (stopped.at ?? ""))
  const startReport = resumedReports.find((entry) => entry.endpoint === "Sessions/Playing")
  ctx.check(startReport && Math.abs(startReport.positionS - resumeAt) < 1, `the start report holds the resume point (${startReport?.positionS.toFixed(1)} s)`)
  const timePos = await ctx.until(async () => {
    const value = await ctx.mpv("time-pos")
    return value > resumeAt - 3 ? value : null
  }, "mpv at the resume point", 15000)
  displayCapture(ctx, "05-resumed")
  await ctx.sleep(11000)
  const early = ctx.playstates().queued.filter((entry) => entry.itemId === id && entry.at > (stopped.at ?? "") && entry.positionS < resumeAt - 3)
  ctx.check(early.length === 0, "no report after Resume went below the resume point")
  report.resume = { launchStartS: launchStart, startReportS: startReport.positionS, mpvTimePosS: timePos, startupCommands: [...new Set(startupCommands)] }

  if (integrated) {
    await ctx.key("q", { code: "KeyQ", keyCode: 81 })
  } else {
    await stopFromBar(ctx)
  }
  await ctx.until(async () => !(await ctx.player())?.active, "playback to stop again", 30000)
  ctx.check((ctx.appLog().match(/mpv IPC connected/g) ?? []).length === 1, "one mpv IPC connection for the whole run")

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
