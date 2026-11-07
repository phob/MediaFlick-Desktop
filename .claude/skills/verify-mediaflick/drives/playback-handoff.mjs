// Playback hand-offs on your own server, through a loopback proxy that can hold
// one title's video stream so its load stays pending in the controller.
//   just verify playback-handoff --account SERVER --signed-out --timeout 600 --log-level "debug,jellyfin.playstate=trace,playback=trace,mpv.ipc=debug"
// Built-in player (the default):
// 1. Mark watched (the `w` key) on a movie, which has no next item: mpv must stop
//    instead of playing on untracked.
// 3. Mark watched on episode 1, so episode 2 loads with its stream held, press
//    `w` again and then `q`: episode 2 must not be marked watched, Stop must end
//    its load at once, and it must never start once its stream is released.
// External mpv (add `--seed .claude/skills/verify-mediaflick/seeds/external-mpv`):
// 2. Play movie A with its stream held, then Play movie B from its page: B must
//    replace the pending load and play; A must never start. Only External mpv
//    leaves the app's pages usable while a title loads, and only the Built-in
//    player bar takes the `w` key, so each backend runs its own phases.
// Writes the test user's playstate and restores every title it played after the run.
const ticksToS = (ticks) => ticks / 1e7
const playable = (item) => {
  const video = item.MediaStreams?.find((stream) => stream.Type === "Video")
  return !item.UserData?.Played && !item.UserData?.PlaybackPositionTicks && video?.Codec === "h264" && video.Height <= 1080
}

function holdGate() {
  let held = null
  const waiting = []
  return {
    hold(id) {
      held = id.toLowerCase()
    },
    release() {
      held = null
      for (const resolve of waiting.splice(0)) resolve()
    },
    delay(url) {
      if (!held || !url.pathname.toLowerCase().includes(`/videos/${held}/`)) return 0
      return new Promise((resolve) => waiting.push(resolve))
    },
  }
}

async function pickMovies(ctx, count) {
  const { Items } = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=Movie&isPlayed=false&sortBy=SortName&fields=MediaStreams&limit=300`)
  const names = Items.map((item) => item.Name)
  const movies = Items.filter((item) => playable(item) && names.indexOf(item.Name) === names.lastIndexOf(item.Name)).slice(0, count)
  if (movies.length < count) throw new Error(`needs ${count} unplayed H.264 movies with unique names and no resume point`)
  return movies
}

async function pickEpisodes(ctx) {
  const userId = ctx.account.userId
  const { Items: shows } = await ctx.jellyfin(`/Items?userId=${userId}&recursive=true&includeItemTypes=Series&isPlayed=false&sortBy=SortName&limit=300`)
  const names = shows.map((show) => show.Name)
  for (const show of shows.filter((candidate) => names.indexOf(candidate.Name) === names.lastIndexOf(candidate.Name))) {
    const { Items: episodes = [] } = await ctx.jellyfin(`/Shows/${show.Id}/Episodes?userId=${userId}&season=1&fields=MediaStreams&limit=3`)
    if (episodes.length === 3 && episodes.every(playable) && episodes.every((episode, index) => episode.IndexNumber === index + 1)) {
      return { series: show.Name, episodes: episodes.map((episode) => ({ id: episode.Id, name: episode.Name, season: episode.ParentIndexNumber })) }
    }
  }
  throw new Error("needs a series whose season 1 opens with three unplayed H.264 episodes")
}

async function openTitle(ctx, name) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${name}` }, { nth: 0 })
}

async function openEpisode(ctx, series, episode) {
  await openTitle(ctx, series)
  await ctx.find({ role: "heading", name: "Episodes" }, { timeout: 30000 })
  await ctx.press({ role: "link", name: episode.name })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${episode.id}`, "the episode page")
}

// Running means mpv has passed two seconds of this title; while it loads, the
// app's position can still be the previous title's.
async function playing(ctx, id, description) {
  await ctx.until(async () => {
    const now = await ctx.player()
    return now?.active && now.itemId === id && new RegExp(`activating pending playback .*launch=item=${id} `).test(ctx.appLog()) && (await ctx.mpv("time-pos").catch(() => 0)) > 2
  }, description, 90000)
  if ((await ctx.mpv("mute")) !== true) {
    await ctx.key("m", { code: "KeyM", keyCode: 77 })
    await ctx.until(async () => (await ctx.mpv("mute")) === true, "mpv muted by the m key")
  }
}

const markWatched = (ctx) => ctx.key("w", { code: "KeyW", keyCode: 87 })
const played = async (ctx, id) => (await ctx.jellyfin(`/Items/${id}?userId=${ctx.account.userId}`)).UserData?.Played === true
const starts = (ctx, id) => ctx.playstates().queued.filter((entry) => entry.endpoint === "Sessions/Playing" && entry.itemId === id)

let expect = () => {}

// 2. Play B while A's load is pending (External mpv).
async function replacePendingLoad(ctx, { gate, proxy, a, b, report }) {
  await openTitle(ctx, a.Name)
  gate.hold(a.Id)
  await ctx.press({ role: "button", name: "Play" })
  await ctx.until(() => proxy.stats.held.some((entry) => entry.path.toLowerCase().includes(`/videos/${a.Id.toLowerCase()}/`)), "A's stream to be held", 60000)
  expect((await ctx.player())?.itemId === a.Id, "A is loading")
  await ctx.screenshot("02-a-loading")
  await openTitle(ctx, b.Name)
  await ctx.press({ role: "button", name: "Play" })
  await ctx.sleep(1500)
  gate.release()
  await playing(ctx, b.Id, "B playing").catch(() => ctx.step("B never played"))
  await ctx.sleep(3000)
  report.replace = { finalItem: (await ctx.player())?.itemId, aStarts: starts(ctx, a.Id).length, bStarts: starts(ctx, b.Id).length }
  expect(report.replace.finalItem === b.Id, "B plays after Play during A's pending load")
  expect(report.replace.aStarts === 0, "A was never reported as started")
  ctx.displayCapture("02-b-playing")
  await stopFromBar(ctx)
  await ctx.until(async () => !(await ctx.player())?.active, "playback to stop", 30000)
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

export default async function playbackHandoff(ctx) {
  if (!ctx.account?.signedOut) throw new Error("playback-handoff needs --account SERVER --signed-out")
  const report = { checks: [] }
  // Every check is recorded and the run fails at the end, so one run shows each phase.
  expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const gate = holdGate()
  const proxy = await ctx.proxy({ upstream: ctx.account.server, delay: gate.delay })
  try {
    await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
    await ctx.quickConnect({ approve: true, timeout: 60000 })
    ctx.revokeAppSessionAfterRun()
    const { client } = (await ctx.api("/api/settings")).body
    const builtIn = client.player.playerBackend === "libmpv"
    report.backend = client.player.playerBackend
    await ctx.until(async () => {
      const now = (await ctx.api("/api/status")).body
      return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
    }, "the first sync to finish", 300000)
    const [movie, a, b] = await pickMovies(ctx, 3)
    const show = await pickEpisodes(ctx)
    for (const id of [movie.Id, a.Id, b.Id, ...show.episodes.map((episode) => episode.id)]) ctx.restoreItemAfterRun(id)
    report.titles = { movie: movie.Name, a: a.Name, b: b.Name, series: show.series, episodes: show.episodes.map((episode) => episode.name) }
    ctx.step(`titles ${JSON.stringify(report.titles)}`)

    if (builtIn) await builtInPhases(ctx, { gate, proxy, movie, show, report })
    else await replacePendingLoad(ctx, { gate, proxy, a, b, report })
  } finally {
    gate.release()
    report.held = proxy.stats.held
    report.playstates = ctx.playstates()
    ctx.writeEvidence("playback-handoff.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} checks failed: ${failed.map((check) => check.message).join("; ")}`)
}

async function builtInPhases(ctx, { gate, proxy, movie, show, report }) {
  // 1. Mark watched with no next item.
  await openTitle(ctx, movie.Name)
  await ctx.press({ role: "button", name: "Play" })
  await playing(ctx, movie.Id, "the movie playing")
  await markWatched(ctx)
  await ctx.until(async () => !(await ctx.player())?.active, "the app to stop tracking the movie", 15000)
  await ctx.until(() => played(ctx, movie.Id), "Jellyfin to mark the movie played", 20000)
  const idle = await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "mpv to go idle after mark watched", 10000).then(() => true, () => false)
  report.markWatched = { mpvIdle: idle, timePos: await ctx.mpv("time-pos").catch(() => null) }
  ctx.displayCapture("01-after-mark-watched")
  expect(idle, "mpv stops after the movie is marked watched")

  // 3. Mark watched again, then Stop, while the next episode loads.
  const [first, second] = show.episodes
  await openEpisode(ctx, show.series, first)
  await ctx.press({ role: "button", name: "Play" })
  await playing(ctx, first.id, "episode 1 playing")
  gate.hold(second.id)
  await markWatched(ctx)
  await ctx.until(() => proxy.stats.held.some((entry) => entry.path.toLowerCase().includes(`/videos/${second.id.toLowerCase()}/`)), "episode 2's stream to be held", 60000)
  await ctx.until(async () => (await ctx.player())?.itemId === second.id, "episode 2 loading", 15000)
  await ctx.until(() => played(ctx, first.id), "Jellyfin to mark episode 1 played", 20000)
  await markWatched(ctx)
  await ctx.sleep(2000)
  await ctx.key("q", { code: "KeyQ", keyCode: 81 })
  const stopped = await ctx.until(async () => !(await ctx.player())?.active, "Stop to end the pending load", 5000).then(() => true, () => false)
  gate.release()
  await ctx.sleep(5000)
  report.whileLoading = {
    secondPlayed: await played(ctx, second.id),
    stopped,
    activeAfterRelease: (await ctx.player())?.active ?? false,
    secondStarts: starts(ctx, second.id).length,
    mpvIdle: await ctx.mpv("idle-active").catch(() => null),
  }
  expect(!report.whileLoading.secondPlayed, "the second press did not mark the loading episode watched")
  expect(stopped, "Stop during the next episode's load ends it at once")
  expect(!report.whileLoading.activeAfterRelease && report.whileLoading.secondStarts === 0 && report.whileLoading.mpvIdle === true, "episode 2 never starts after its stream is released")
  ctx.displayCapture("03-stopped-while-loading")
}
