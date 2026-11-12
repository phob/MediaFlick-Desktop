// What Jellyfin keeps after the playback cases the `playback` drive does not
// cover, read back from Jellyfin itself after each case:
// - play-to-end: a movie plays to its end: played, resume point cleared.
// - finish-episode: episode 1 plays to its end, episode 2 follows: episode 1
//   played, Next Up moves to episode 2.
// - stop-early: episode 2 played and stopped within its first seconds: not
//   played, no resume point.
// - sign-out (External mpv only; the Built-in player covers the page, so the
//   user menu is out of reach while it plays): sign out while a movie plays,
//   then sign in to the demo server: playback ends and nothing is reported
//   once the other account is signed in.
// A case that needs the end of a title sets the resume point 40 s before it in
// Jellyfin first, as another client would; the app then plays the end itself.
// Writes the test user's playstate and puts it back; signs both of the app's
// tokens out.
//   just verify playstate --account SERVER --signed-out --timeout 900 --log-level "debug,jellyfin.playstate=trace,playback=trace"
// External mpv: add `--seed .claude/skills/verify-mediaflick/seeds/external-mpv`.
const TAIL_S = 40
const DEMO = { server: "https://demo.jellyfin.org/stable", username: "demo" }
const RESUME = { role: "button", name: "Resume" }
const SERVER = { role: "textbox", name: "Server" }
const status = async (ctx) => (await ctx.api("/api/status")).body
const ticksToS = (ticks) => ticks / 1e7
const logSince = (ctx, marker) => ctx.appLog().slice(marker)
const userMenu = (user, server) => ({ role: "button", name: `${user[0].toUpperCase()} ${user} ${new URL(server).host}` })
const playable = (item) => {
  const video = item.MediaStreams?.find((stream) => stream.Type === "Video")
  return !item.UserData?.Played && !item.UserData?.PlaybackPositionTicks && video?.Codec === "h264" && video.Height <= 1080 && item.RunTimeTicks > 10 * 60 * 1e7
}
const title = (item, extra = {}) => ({ id: item.Id, name: item.Name, runtimeTicks: item.RunTimeTicks, ...extra })

async function pickMovies(ctx, count) {
  const { Items } = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&isPlayed=false&sortBy=SortName&fields=MediaStreams&limit=300`)
  const movies = Items.filter(playable).slice(0, count)
  if (movies.length < count) throw new Error(`need ${count} unplayed H.264 movies without a resume point, found ${movies.length}`)
  return movies.map((movie) => title(movie))
}

async function pickEpisodes(ctx) {
  const userId = ctx.account.userId
  const { Items: shows } = await ctx.jellyfin(`/Items?userId=${userId}&recursive=true&includeItemTypes=Series&isPlayed=false&sortBy=SortName&limit=300`)
  const names = shows.map((show) => show.Name)
  for (const show of shows.filter((candidate) => names.indexOf(candidate.Name) === names.lastIndexOf(candidate.Name))) {
    const { Items: episodes = [] } = await ctx.jellyfin(`/Shows/${show.Id}/Episodes?userId=${userId}&season=1&fields=MediaStreams&limit=2`)
    if (episodes.length === 2 && episodes.every(playable) && episodes[0].IndexNumber === 1 && episodes[1].IndexNumber === 2) {
      return { series: { id: show.Id, name: show.Name }, episodes: episodes.map((episode) => title(episode, { season: episode.ParentIndexNumber })) }
    }
  }
  throw new Error("no series whose first two season-1 episodes are unplayed H.264 without a resume point")
}

const userData = async (ctx, id) => (await ctx.jellyfin(`/Items/${id}?userId=${ctx.account.userId}`)).UserData
const nextUp = async (ctx, seriesId) => (await ctx.jellyfin(`/Shows/NextUp?userId=${ctx.account.userId}&seriesId=${seriesId}`)).Items?.[0]?.Id ?? null
const brief = (data) => ({ played: data.Played, positionS: ticksToS(data.PlaybackPositionTicks ?? 0), playCount: data.PlayCount })

async function openMovie(ctx, movie) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, movie.name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${movie.name}` }, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${movie.id}`, `the page of ${movie.name}`)
}

async function openEpisode(ctx, series, episode) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, series.name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${series.name}` }, { nth: 0 })
  await ctx.find({ role: "heading", name: "Episodes" }, { timeout: 30000 })
  const season = { role: "button", name: `Season ${episode.season}` }
  if ((await ctx.exists(season)) && (await ctx.state(season)).pressed !== true) await ctx.press(season)
  await ctx.press({ role: "link", name: episode.name })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${episode.id}`, "the episode page")
}

async function playing(ctx, id, fromMs, description) {
  await ctx.until(async () => {
    const now = await ctx.player()
    return now?.active && now.itemId === id && now.positionMs > fromMs
  }, description, 90000)
  if ((await ctx.mpv("mute")) !== true) {
    await ctx.key("m", { code: "KeyM", keyCode: 77 })
    await ctx.until(async () => (await ctx.mpv("mute")) === true, "mpv muted by the m key")
  }
}

// The player bar shows its own Stop from 1536 px; narrower, Stop moves into
// the overflow menu.
async function stopFromBar(ctx) {
  const stop = { role: "button", name: "Stop" }
  const more = { role: "button", name: "More playback controls" }
  await ctx.revealPlayer()
  const direct = await ctx.until(async () => ((await ctx.exists(stop)) ? "direct" : (await ctx.exists(more)) ? "menu" : null), "the player bar's stop control", 15000)
  if (direct === "direct") return ctx.press(stop)
  await ctx.press(more)
  await ctx.press({ role: "menuitem", name: "Stop playback" })
}

async function stopPlayback(ctx, integrated) {
  if (!(await ctx.player())?.active) return
  if (integrated) await ctx.key("q", { code: "KeyQ", keyCode: 81 })
  else await stopFromBar(ctx)
  await ctx.until(async () => !(await ctx.player())?.active, "playback to stop", 30000)
}

const stoppedReports = (ctx, id) => ctx.playstates().queued.filter((entry) => entry.endpoint === "Sessions/Playing/Stopped" && entry.itemId === id)
const stopAccepted = (ctx, id) => ctx.until(() => ctx.playstates().sent.some((entry) => entry.endpoint === "Sessions/Playing/Stopped" && entry.itemId === id && entry.status === 204), "Jellyfin to accept the Stopped report (204)", 20000)

async function playToEnd(ctx, movie, expectIn) {
  const expect = expectIn("play-to-end")
  await ctx.setResumeOnServer(movie.id, movie.runtimeTicks - TAIL_S * 1e7)
  const before = brief(await userData(ctx, movie.id))
  await openMovie(ctx, movie)
  await ctx.find(RESUME, { timeout: 30000 })
  const marker = ctx.appLog().length
  await ctx.press(RESUME)
  await playing(ctx, movie.id, (before.positionS + 1) * 1000, "the movie playing from its resume point")
  await ctx.until(async () => !(await ctx.player())?.active, "the movie to play to its end", (TAIL_S + 60) * 1000)
  expect(/reporting active playback stopped failed=false reason="eof"/.test(logSince(ctx, marker)), "the app reported the stop with reason eof")
  const stopped = stoppedReports(ctx, movie.id).at(-1)
  expect(stopped && stopped.positionS > ticksToS(movie.runtimeTicks) - 5, `the Stopped report sits at the end (${stopped?.positionS.toFixed(1)} of ${ticksToS(movie.runtimeTicks).toFixed(1)} s)`)
  await stopAccepted(ctx, movie.id)
  const after = brief(await ctx.until(async () => {
    const data = await userData(ctx, movie.id)
    return data.Played ? data : null
  }, "Jellyfin to mark the movie played", 20000).catch(() => userData(ctx, movie.id)))
  expect(after.played === true && after.positionS === 0, `Jellyfin holds it played with no resume point (${JSON.stringify(after)})`)
  return { "play-to-end": { items: { [movie.id]: { name: movie.name, before, after } } } }
}

// Episode 1 plays to its end and episode 2 follows. Episode 2 is stopped at
// once: under Xvfb, External mpv hangs on a second load in the same process
// (see the segments drive), so stop-early plays it in a pass of its own.
async function finishEpisode(ctx, { series, episodes: [first, second] }, integrated, expectIn) {
  const expect = expectIn("finish-episode")
  await ctx.setResumeOnServer(first.id, first.runtimeTicks - TAIL_S * 1e7)
  const before = { ...brief(await userData(ctx, first.id)), nextUp: await nextUp(ctx, series.id) }
  await openEpisode(ctx, series, first)
  await ctx.find(RESUME, { timeout: 30000 })
  const marker = ctx.appLog().length
  await ctx.press(RESUME)
  await playing(ctx, first.id, (before.positionS + 1) * 1000, "episode 1 playing from its resume point")

  // Next episode follows on its own by default; with a countdown, Play now.
  const playNow = { role: "button", name: "Play now" }
  await ctx.until(async () => {
    const now = await ctx.player()
    if (now?.active && now.itemId === second.id) return true
    if (await ctx.exists(playNow)) await ctx.press(playNow)
    return false
  }, "episode 2 to follow episode 1", (TAIL_S + 90) * 1000)
  expect(/reporting active playback stopped failed=false reason="eof"/.test(logSince(ctx, marker)), "the app reported episode 1's stop with reason eof")
  await stopAccepted(ctx, first.id)
  await stopPlayback(ctx, integrated).catch((error) => ctx.step(`episode 2 did not stop cleanly: ${error.message}`))

  const after = { ...brief(await userData(ctx, first.id)) }
  after.nextUp = await ctx.until(async () => {
    const id = await nextUp(ctx, series.id)
    return id === second.id ? id : null
  }, "Next Up to move to episode 2", 20000).catch(() => nextUp(ctx, series.id))
  expect(after.played === true && after.positionS === 0, `Jellyfin holds episode 1 played with no resume point (${JSON.stringify(after)})`)
  expect(after.nextUp === second.id, `Next Up for ${series.name} is episode 2 (${after.nextUp})`)
  return { "finish-episode": { items: { [first.id]: { name: first.name, before, after } } } }
}

async function stopEarly(ctx, { series, episodes: [, episode] }, integrated, expectIn) {
  const expect = expectIn("stop-early")
  const before = brief(await userData(ctx, episode.id))
  await openEpisode(ctx, series, episode)
  const fromStart = { role: "button", name: "From start" }
  await ctx.until(async () => (await ctx.exists(fromStart)) || (await ctx.exists({ role: "button", name: "Play" })), "Play or From start")
  await ctx.press((await ctx.exists(fromStart)) ? fromStart : { role: "button", name: "Play" })
  await playing(ctx, episode.id, 2000, "episode 2 playing")
  await stopPlayback(ctx, integrated)
  const stopped = stoppedReports(ctx, episode.id).at(-1)
  expect(stopped && stopped.positionS < 15, `episode 2 stopped within its first seconds (${stopped?.positionS.toFixed(1)} s)`)
  await stopAccepted(ctx, episode.id)
  const after = brief(await userData(ctx, episode.id))
  expect(after.played === false && after.positionS === 0, `Jellyfin holds episode 2 unplayed with no resume point (${JSON.stringify(after)})`)
  return { "stop-early": { items: { [episode.id]: { name: episode.name, before, after } } } }
}

async function signOutWhilePlaying(ctx, movie, expectIn) {
  const expect = expectIn("sign-out")
  const before = brief(await userData(ctx, movie.id))
  const { deviceId } = await status(ctx)
  await openMovie(ctx, movie)
  await ctx.press({ role: "button", name: "Play" })
  await playing(ctx, movie.id, 5000, "the movie playing")
  await ctx.until(() => ctx.playstates().sent.some((entry) => entry.endpoint === "Sessions/Playing" && entry.itemId === movie.id && entry.status === 204), "Jellyfin to accept the start report")

  const reportsBefore = ctx.playstates().queued.length
  await ctx.press(userMenu(ctx.account.userName, ctx.account.server))
  await ctx.press({ role: "menuitem", name: "Sign out" })
  await ctx.find(SERVER, { timeout: 30000 })
  await ctx.until(async () => !(await ctx.player())?.active, "playback to end with the sign-out", 15000)
  const signedOut = ctx.playstates().queued
  const duringSignOut = signedOut.slice(reportsBefore)
  expect(duringSignOut.every((entry) => entry.itemId === movie.id) && duringSignOut.filter((entry) => entry.endpoint === "Sessions/Playing").length === 0, `the sign-out reported only the end of this movie (${duringSignOut.map((entry) => entry.endpoint).join(", ") || "nothing"})`)

  await ctx.fill(SERVER, DEMO.server)
  await ctx.fill({ role: "textbox", name: "Username" }, DEMO.username)
  await ctx.press({ role: "button", name: "Sign in" })
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  ctx.step(`signed in to ${DEMO.server} as ${DEMO.username}; waiting 15 s for any late report`)
  await ctx.sleep(15000)
  const late = ctx.playstates().queued.slice(signedOut.length)
  expect(late.length === 0, `nothing was reported after the other account signed in (${late.length} reports)`)
  const sessions = await ctx.jellyfin(`/Sessions?deviceId=${encodeURIComponent(deviceId)}`)
  expect(!sessions.some((entry) => entry.NowPlayingItem), "Jellyfin shows no session of the app still playing")
  const after = brief(await userData(ctx, movie.id))

  await ctx.press(userMenu(DEMO.username, DEMO.server))
  await ctx.press({ role: "menuitem", name: "Sign out" })
  await ctx.find(SERVER, { timeout: 30000 })
  return { "sign-out": { items: { [movie.id]: { name: movie.name, before, after } }, reports: { duringSignOut, afterSwitch: late } } }
}

export default async function playstate(ctx) {
  if (!ctx.account?.signedOut) throw new Error("playstate needs --account SERVER --signed-out: the sign-out case revokes the app's own token")
  // Without an audio device mpv ends every file with reason error instead of
  // eof, even after playing it to the end.
  if (process.platform === "linux") await ctx.audioSink()
  await ctx.signIn({})
  ctx.revokeAppSessionAfterRun()
  await ctx.until(async () => {
    const now = await status(ctx)
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  const { client, capabilities } = (await ctx.api("/api/settings")).body
  const integrated = capabilities.integratedLibmpvOverlay === true
  const [endMovie, signOutMovie] = await pickMovies(ctx, 2)
  const show = await pickEpisodes(ctx)
  for (const id of [endMovie.id, signOutMovie.id, ...show.episodes.map((episode) => episode.id)]) ctx.restoreItemAfterRun(id)
  const report = { backend: client.player.playerBackend, integrated, platform: process.platform, cases: [] }
  ctx.step(`backend ${report.backend}${integrated ? " (integrated overlay)" : ""}; movies ${endMovie.name}, ${signOutMovie.name}; ${show.series.name} E1/E2`)
  await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "mpv warm and idle")

  const failures = []
  // Runs one pass of the app that covers the named cases and records each
  // with its own checks. External mpv under Xvfb hangs on a second load after
  // a stop in the same process (see the segments drive), so every pass after
  // the first relaunches it for a fresh mpv.
  const run = async (names, body) => {
    if (!integrated && report.cases.length) {
      await ctx.relaunch()
      await ctx.until(async () => (await status(ctx))?.bootstrap?.state === "ready", "the app ready again after the relaunch", 60000)
      await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "the new mpv warm and idle")
    }
    const checks = Object.fromEntries(names.map((name) => [name, []]))
    const expectIn = (name) => (ok, message) => {
      checks[name].push({ ok: Boolean(ok), message })
      ctx.step(`${ok ? "ok" : "FAIL"}: ${name}: ${message}`)
    }
    let results = {}
    try {
      results = await body(expectIn)
    } catch (error) {
      ctx.step(`${names.join(" and ")} FAILED: ${error.message}`)
      await ctx.screenshot(`${names[0]}-failure`)
      await ctx.snapshot(`${names[0]}-failure`)
      await stopPlayback(ctx, integrated).catch(() => {})
      for (const name of names) if (!results[name]) checks[name].push({ ok: false, message: `did not finish: ${error.message}` })
    }
    for (const name of names) {
      const passed = checks[name].length > 0 && checks[name].every((check) => check.ok)
      if (!passed) failures.push(name)
      report.cases.push({ name, ...results[name], checks: checks[name], passed })
    }
    ctx.writeEvidence("playstate.json", report)
  }

  await run(["play-to-end"], (expectIn) => playToEnd(ctx, endMovie, expectIn))
  await run(["finish-episode"], (expectIn) => finishEpisode(ctx, show, integrated, expectIn))
  await run(["stop-early"], (expectIn) => stopEarly(ctx, show, integrated, expectIn))
  if (integrated) {
    report.cases.push({ name: "sign-out", skipped: "the Built-in player covers the page, so the user menu cannot be reached while it plays" })
    ctx.step("sign-out skipped: Built-in player")
  } else {
    await run(["sign-out"], (expectIn) => signOutWhilePlaying(ctx, signOutMovie, expectIn))
  }
  ctx.writeEvidence("playstate.json", report)
  if (failures.length) throw new Error(`playstate cases failed: ${failures.join(", ")}`)
}
