// Media segments on your own server, which has none: a loopback proxy in
// front of it forwards everything and answers `GET /MediaSegments/<id>` for
// two episodes, one with embedded chapters and one without, with an Intro
// (6.25–97.5 s) and an Outro near the end. Each case saves the Playback shelf's
// four segment selects through Settings, plays an episode from the start and,
// inside the intro, reads `/api/player/state`'s skipSegments, the injected
// markers in mpv's chapter-list, the player bar's `Skip Intro`, mpv's OSD
// prompt (raw `show-text` commands in app.log) and the position running on
// through the intro. With External mpv the page stays usable, so one more case
// switches Intro to Don't show and saves mid-playback.
// Plays real audio and writes the test user's playstate; leaves both episodes
// unplayed without a resume point and signs the app's own token out.
//   just verify segments --account https://jellyfin.example.com --signed-out --timeout 600 --log-level "debug,mpv.ipc=trace"
// External mpv: add `--seed .claude/skills/verify-mediaflick/seeds/external-mpv`.
// MEDIAFLICK_VERIFY_SEGMENTS=baseline runs only the cases that predate the
// Don't show option (Ask me, Never); the report lists the others as skipped.
import { mediaSegmentsAnswer } from "../scripts/proxy.mjs"

const ASK = "Ask me"
const NEVER = "Never"
const HIDDEN = "Don't show"
const MODES = { Never: "disabled", "Ask me": "prompt", "Always skip": "always", "Don't show": "hidden" }
const SELECTS = [
  { label: "Intro skipping", key: "skipIntro" },
  { label: "Credits skipping", key: "skipCredits" },
  { label: "Recap skipping", key: "skipRecap" },
  { label: "Commercial skipping", key: "skipCommercial" },
]
// Off round seconds: the app's merge keeps an embedded chapter at the same
// time instead of the marker, and files often have chapters on round seconds.
const INTRO = { startS: 6.25, endS: 97.5 }
const OUTRO_BEFORE_END = { startS: 87.5, endS: 12.5 }
const all = (mode) => [mode, mode, mode, mode]

// `episode`: `chapters` has embedded chapters the markers merge into, `plain`
// has none. `show`: the segment types expected in skipSegments and as chapter
// markers; the Skip Intro button follows `intro`. `fetch: false` expects no
// MediaSegments request at all.
const CASES = [
  { name: "intro-ask", baseline: true, episode: "chapters", selects: all(ASK), show: { intro: true, outro: true }, prompt: true, pressSkip: true },
  { name: "intro-never", baseline: true, episode: "chapters", selects: [NEVER, ASK, ASK, ASK], show: { intro: true, outro: true }, prompt: false },
  { name: "plain-ask", baseline: true, episode: "plain", selects: all(ASK), show: { intro: true, outro: true }, prompt: true },
  { name: "intro-hidden", episode: "chapters", selects: [HIDDEN, ASK, ASK, ASK], show: { intro: false, outro: true }, prompt: false },
  { name: "all-never", episode: "chapters", selects: all(NEVER), show: { intro: true, outro: true }, prompt: false },
  { name: "all-hidden", episode: "chapters", selects: all(HIDDEN), show: { intro: false, outro: false }, prompt: false, fetch: false },
  { name: "live-hidden", externalOnly: true, episode: "chapters", selects: all(ASK), show: { intro: true, outro: true }, prompt: true, liveSwitch: [HIDDEN, ASK, ASK, ASK] },
]

const status = async (ctx) => (await ctx.api("/api/status")).body
const ticksToS = (ticks) => ticks / 1e7
const logSince = (ctx, marker) => ctx.appLog().slice(marker)
const segmentTypes = (state) => (state?.skipSegments ?? []).map((segment) => segment.segmentType).sort()
const promptShown = (log) => /sending raw mpv command command=.*"show-text","Seek to Skip Intro"/.test(log)
const mpvConnections = (ctx) => (ctx.appLog().match(/mpv IPC connected/g) ?? []).length

function markersFor(episode, type) {
  const segment = episode.segments.find((candidate) => candidate.type.toLowerCase() === type)
  const [start, end] = type === "intro" ? ["Intro", "Intro End"] : ["Credits", "Credits End"]
  return [{ title: start, time: segment.startS }, { title: end, time: segment.endS }]
}
const hasMarker = (chapters, marker) => chapters.some((chapter) => chapter.title === marker.title && Math.abs(chapter.time - marker.time) < 0.01)

async function settle(ctx, condition, description, timeout) {
  try {
    return await ctx.until(condition, description, timeout)
  } catch {
    ctx.step(`gave up waiting for ${description}`)
    return null
  }
}

function displayCapture(ctx, name) {
  if (process.platform === "linux") ctx.displayCapture(name)
  else ctx.step(`no display capture for ${name}: Linux only`)
}

async function pickEpisode(ctx, withChapters) {
  const userId = ctx.account.userId
  const { Items: shows } = await ctx.jellyfin(`/Items?userId=${userId}&recursive=true&includeItemTypes=Series&isPlayed=false&sortBy=SortName&limit=300`)
  const names = shows.map((show) => show.Name)
  for (const show of shows.filter((candidate) => names.indexOf(candidate.Name) === names.lastIndexOf(candidate.Name))) {
    const { Items: [episode] = [] } = await ctx.jellyfin(`/Shows/${show.Id}/Episodes?userId=${userId}&season=1&fields=MediaStreams,MediaSources,Chapters&limit=1`)
    const video = episode?.MediaStreams?.find((stream) => stream.Type === "Video")
    const runtimeS = ticksToS(episode?.RunTimeTicks ?? 0)
    const chapters = episode?.Chapters?.length ?? 0
    if (episode && !episode.UserData?.Played && !episode.UserData?.PlaybackPositionTicks && runtimeS >= 15 * 60 && video?.Codec === "h264" && video.Height <= 1080 && (withChapters ? chapters > 1 : chapters === 0)) {
      return {
        series: show.Name,
        id: episode.Id,
        name: episode.Name,
        season: episode.ParentIndexNumber,
        index: episode.IndexNumber,
        runtimeS,
        embeddedChapters: chapters,
        sourceIds: (episode.MediaSources ?? []).map((source) => source.Id),
        segments: [{ type: "Intro", ...INTRO }, { type: "Outro", startS: runtimeS - OUTRO_BEFORE_END.startS, endS: runtimeS - OUTRO_BEFORE_END.endS }],
      }
    }
  }
  throw new Error(`no series whose S1E1 is an unplayed H.264 episode of 15 minutes or more ${withChapters ? "with" : "without"} embedded chapters`)
}

async function openSettingsShelf(ctx) {
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Playback" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/playback", "the Playback shelf")
  await ctx.find({ role: "combobox", name: "Intro skipping" }, { timeout: 30000 })
}

async function saveSelects(ctx, modes) {
  for (const [index, { label }] of SELECTS.entries()) {
    if ((await ctx.state({ role: "combobox", name: label })).value !== modes[index]) await ctx.choose({ name: label }, modes[index])
  }
  if ((await ctx.state({ role: "button", name: "Save" })).disabled !== true) {
    await ctx.press({ role: "button", name: "Save" })
    await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save to finish")
  }
  const playback = (await ctx.api("/api/settings")).body.client.playback
  const stored = Object.fromEntries(SELECTS.map(({ key }) => [key, playback[key]]))
  ctx.check(SELECTS.every(({ key }, index) => stored[key] === MODES[modes[index]]), `the app stores ${JSON.stringify(stored)}`)
  return stored
}

async function openEpisode(ctx, episode) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, episode.series)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.press({ role: "link", name: `Open details for ${episode.series}` }, { nth: 0 })
  await ctx.find({ role: "heading", name: "Episodes" }, { timeout: 30000 })
  const season = { role: "button", name: `Season ${episode.season}` }
  if ((await ctx.exists(season)) && (await ctx.state(season)).pressed !== true) await ctx.press(season)
  await ctx.press({ role: "link", name: episode.name })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${episode.id}`, "the episode page")
}

async function startFromBeginning(ctx, episode) {
  const fromStart = { role: "button", name: "From start" }
  await ctx.until(async () => (await ctx.exists(fromStart)) || (await ctx.exists({ role: "button", name: "Play" })), "Play or From start")
  await ctx.press((await ctx.exists(fromStart)) ? fromStart : { role: "button", name: "Play" })
  await ctx.until(async () => {
    const now = await ctx.player()
    return now?.active && now.itemId === episode.id && now.positionMs > (INTRO.startS + 2) * 1000
  }, "playback inside the intro", 90000)
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

async function skipButtonShown(ctx) {
  await ctx.revealPlayer()
  await ctx.find({ role: "button", name: "Pause" }, { timeout: 10000 })
  return ctx.exists({ role: "button", name: "Skip Intro" })
}

async function runCase(ctx, spec, { episode, integrated, proxy, report }) {
  const result = { name: spec.name, episode: episode.id, selects: spec.selects, checks: [] }
  report.cases.push(result)
  const expect = (ok, message) => {
    result.checks.push({ ok: Boolean(ok), message })
    ctx.step(`${ok ? "ok" : "FAIL"}: ${message}`)
  }
  await openSettingsShelf(ctx)
  result.stored = await saveSelects(ctx, spec.selects)
  await openEpisode(ctx, episode)
  const marker = ctx.appLog().length
  const answeredBefore = proxy.stats.answered.length
  await startFromBeginning(ctx, episode)

  const wanted = Object.keys(spec.show).filter((type) => spec.show[type]).sort()
  const present = wanted.flatMap((type) => markersFor(episode, type))
  const absent = Object.keys(spec.show).filter((type) => !spec.show[type]).flatMap((type) => markersFor(episode, type))
  if (spec.fetch === false) await ctx.sleep(3000)
  else {
    await settle(ctx, async () => {
      const listed = (await ctx.mpv("chapter-list")) ?? []
      return JSON.stringify(segmentTypes(await ctx.player())) === JSON.stringify(wanted) && present.every((item) => hasMarker(listed, item))
    }, `skipSegments ${JSON.stringify(wanted)} and their chapter markers`, 20000)
  }
  const state = await ctx.player()
  const chapters = (await ctx.mpv("chapter-list")) ?? []
  Object.assign(result, { skipSegments: state.skipSegments, chapters })
  expect(JSON.stringify(segmentTypes(state)) === JSON.stringify(wanted), `skipSegments holds ${JSON.stringify(segmentTypes(state))}, expected ${JSON.stringify(wanted)}`)
  expect(present.every((item) => hasMarker(chapters, item)), `mpv's chapter-list holds the markers ${JSON.stringify(present.map((item) => `${item.title}@${item.time}`))} (${chapters.length} chapters: ${JSON.stringify(chapters.map((chapter) => chapter.title))})`)
  expect(!absent.some((item) => hasMarker(chapters, item)), `mpv's chapter-list has none of ${JSON.stringify(absent.map((item) => `${item.title}@${item.time}`))}`)

  const fetched = /fetching Jellyfin media segments/.test(logSince(ctx, marker))
  const answered = proxy.stats.answered.slice(answeredBefore).length
  expect(fetched === (spec.fetch !== false) && answered > 0 === (spec.fetch !== false), `the app asked for media segments: ${fetched} (proxy answered ${answered}), expected ${spec.fetch !== false}`)

  result.button = await skipButtonShown(ctx)
  expect(result.button === spec.show.intro, `the bar ${result.button ? "shows" : "has no"} Skip Intro, expected ${spec.show.intro ? "shown" : "none"}`)
  await ctx.snapshot(`${spec.name}-bar`)
  await ctx.screenshot(`${spec.name}-bar`)
  displayCapture(ctx, `${spec.name}-display`)

  const samples = [(await ctx.player()).positionMs]
  for (let n = 0; n < 6; n++) {
    await ctx.sleep(1000)
    samples.push((await ctx.player()).positionMs)
  }
  result.positionsMs = samples
  const steps = samples.slice(1).map((value, n) => value - samples[n])
  expect(steps.every((step) => step > 0 && step < 3000) && samples.at(-1) < INTRO.endS * 1000, `the position runs on through the intro without a skip (${samples.map((value) => (value / 1000).toFixed(1)).join(" → ")} s)`)
  result.prompt = promptShown(logSince(ctx, marker))
  expect(result.prompt === spec.prompt, `mpv ${result.prompt ? "showed" : "did not show"} the OSD prompt Seek to Skip Intro, expected ${spec.prompt ? "shown" : "none"}`)

  if (spec.pressSkip && result.button) {
    await ctx.revealPlayer()
    await ctx.press({ role: "button", name: "Skip Intro" })
    result.skippedToMs = await settle(ctx, async () => {
      const now = (await ctx.player()).positionMs
      return now >= INTRO.endS * 1000 - 1000 ? now : null
    }, "the position at the end of the intro", 15000)
    expect(result.skippedToMs !== null && !(await skipButtonShown(ctx)), `Skip Intro jumped to ${((result.skippedToMs ?? NaN) / 1000).toFixed(1)} s and left the bar`)
  }

  if (spec.liveSwitch) {
    const connections = mpvConnections(ctx)
    const switchMarker = ctx.appLog().length
    await openSettingsShelf(ctx)
    result.liveStored = await saveSelects(ctx, spec.liveSwitch)
    await settle(ctx, async () => {
      const titles = (await ctx.mpv("chapter-list")) ?? []
      return !segmentTypes(await ctx.player()).includes("intro") && !markersFor(episode, "intro").some((item) => hasMarker(titles, item)) && !(await ctx.exists({ role: "button", name: "Skip Intro" }))
    }, "the intro to leave skipSegments, the chapter markers and the bar", 15000)
    const after = await ctx.player()
    const afterChapters = (await ctx.mpv("chapter-list")) ?? []
    result.live = { skipSegments: after.skipSegments, chapters: afterChapters, positionMs: after.positionMs, button: await ctx.exists({ role: "button", name: "Skip Intro" }) }
    expect(!segmentTypes(after).includes("intro") && segmentTypes(after).includes("outro"), `after saving, skipSegments holds ${JSON.stringify(segmentTypes(after))}, expected ["outro"]`)
    expect(!markersFor(episode, "intro").some((item) => hasMarker(afterChapters, item)) && markersFor(episode, "outro").every((item) => hasMarker(afterChapters, item)), "after saving, the Intro markers are gone and the Credits markers stay")
    expect(!result.live.button, "after saving, the bar shows no Skip Intro")
    expect(after.active && after.itemId === episode.id && after.positionMs < INTRO.endS * 1000, `the same playback runs on inside the intro (${(after.positionMs / 1000).toFixed(1)} s)`)
    expect(!/starting playback/.test(logSince(ctx, switchMarker)) && mpvConnections(ctx) === connections, "saving did not restart playback or mpv")
    await ctx.snapshot(`${spec.name}-after-save`)
    await ctx.screenshot(`${spec.name}-after-save`)
    displayCapture(ctx, `${spec.name}-after-save-display`)
  }
  await stopPlayback(ctx, integrated)
  const failed = result.checks.filter((entry) => !entry.ok)
  result.passed = failed.length === 0
  if (failed.length) throw new Error(failed.map((entry) => entry.message).join("; "))
}

async function userData(ctx, id) {
  return (await ctx.jellyfin(`/Items/${id}?userId=${ctx.account.userId}`)).UserData
}

async function restoreEpisode(ctx, episode) {
  const before = await userData(ctx, episode.id)
  if (!before.Played && !before.PlaybackPositionTicks) {
    ctx.step(`${episode.name} is still unplayed without a resume point`)
    return before
  }
  await openEpisode(ctx, episode)
  await ctx.press({ role: "button", name: "Mark watched" })
  await ctx.find({ role: "button", name: "Mark unwatched" }, { timeout: 15000 })
  await ctx.until(async () => (await userData(ctx, episode.id)).Played === true, "Jellyfin to mark it played")
  await ctx.press({ role: "button", name: "Mark unwatched" })
  await ctx.find({ role: "button", name: "Mark watched" }, { timeout: 15000 })
  const final = await ctx.until(async () => {
    const data = await userData(ctx, episode.id)
    return data.Played === false ? data : null
  }, "Jellyfin to mark it unplayed")
  ctx.check(!final.PlaybackPositionTicks, `${episode.name} is unplayed with no resume point again`)
  return final
}

export default async function segments(ctx) {
  if (!ctx.account?.signedOut) throw new Error("segments needs --account SERVER --signed-out: the app signs in to the proxy with its own token")
  const baseline = process.env.MEDIAFLICK_VERIFY_SEGMENTS === "baseline"
  const episodes = { chapters: await pickEpisode(ctx, true), plain: await pickEpisode(ctx, false) }
  for (const [kind, episode] of Object.entries(episodes)) {
    ctx.step(`${kind} episode: ${episode.series} S${episode.season}E${episode.index} ${episode.name} (${episode.id}), ${Math.round(episode.runtimeS / 60)} min, ${episode.embeddedChapters} embedded chapters`)
  }
  const answers = Object.values(episodes).flatMap((episode) => [episode.id, ...episode.sourceIds].map((id) => [id, episode.segments]))
  const proxy = await ctx.proxy({ upstream: ctx.account.server, answer: mediaSegmentsAnswer(Object.fromEntries(answers)) })

  await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
  await ctx.quickConnect({ approve: true, timeout: 60000 })
  ctx.revokeAppSessionAfterRun()
  for (const episode of Object.values(episodes)) ctx.restoreItemAfterRun(episode.id)
  await ctx.until(async () => {
    const now = await status(ctx)
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync through the proxy to finish", 300000)
  ctx.check(proxy.stats.forwarded > 0, `the app reaches Jellyfin through the proxy (${proxy.stats.forwarded} requests so far)`)

  const { client, capabilities } = (await ctx.api("/api/settings")).body
  const integrated = capabilities.integratedLibmpvOverlay === true
  const report = { backend: client.player.playerBackend, integrated, mode: baseline ? "baseline" : "all", episodes, cases: [], skipped: [] }
  ctx.step(`backend ${report.backend}${integrated ? " (integrated overlay)" : ""}, cases: ${report.mode}`)
  await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "mpv warm and idle")

  const failures = []
  for (const spec of CASES) {
    if ((baseline && !spec.baseline) || (spec.externalOnly && integrated)) {
      report.skipped.push({ name: spec.name, reason: baseline && !spec.baseline ? "baseline run" : "Settings is unreachable while the Built-in player covers the page" })
      continue
    }
    ctx.step(`case ${spec.name}: ${spec.selects.join(" / ")} on the ${spec.episode} episode`)
    // External mpv 0.41 under Xvfb hangs on a second loadfile after stop in
    // the same process (bare mpv reproduces it, with or without the proxy);
    // the app only recovers after mpv's 60 s network timeout. A relaunch gives
    // each case a fresh mpv.
    if (!integrated && report.cases.length) {
      ctx.step("relaunching for a fresh External mpv: a second load in the same mpv hangs under Xvfb")
      await ctx.relaunch()
      await ctx.until(async () => (await status(ctx))?.bootstrap?.state === "ready", "the app ready again after the relaunch", 60000)
      await ctx.until(async () => (await ctx.mpv("idle-active")) === true, "the new mpv warm and idle")
    }
    try {
      await runCase(ctx, spec, { episode: episodes[spec.episode], integrated, proxy, report })
    } catch (error) {
      const result = report.cases.at(-1)
      result.passed = false
      result.error = error.message
      failures.push(`${spec.name}: ${error.message}`)
      ctx.step(`case ${spec.name} FAILED: ${error.message}`)
      await ctx.screenshot(`${spec.name}-failure`)
      await ctx.snapshot(`${spec.name}-failure`)
      await ctx.key("Escape", { code: "Escape", keyCode: 27 })
      await stopPlayback(ctx, integrated)
    }
    ctx.writeEvidence("segments.json", { ...report, proxy: proxy.stats })
  }

  await openSettingsShelf(ctx)
  await saveSelects(ctx, all(ASK))
  report.cleanup = {}
  for (const episode of Object.values(episodes)) {
    const final = await restoreEpisode(ctx, episode)
    report.cleanup[episode.id] = { played: final.Played, positionTicks: final.PlaybackPositionTicks }
  }
  ctx.writeEvidence("segments.json", { ...report, proxy: proxy.stats })
  if (failures.length) throw new Error(`segment cases failed:\n${failures.join("\n")}`)
}
