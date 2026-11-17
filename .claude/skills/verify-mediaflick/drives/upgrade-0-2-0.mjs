// One start on a profile MediaFlick 0.2.0 made, as `scripts/e2e/upgrade-0-2-0.mjs`
// runs it: the update to this build, the way back to the real 0.2.0, and the
// update again. Run that script rather than this drive alone; it passes:
//
//   MEDIAFLICK_VERIFY_UPGRADE_BEFORE   before.json: what the profile held before this start
//   MEDIAFLICK_VERIFY_UPGRADE_PRESYNC  pre-sync.json: the script's comparison of library.db
//                                      after the same binary opened a copy of the profile with
//                                      --library-stats, which migrates and exits before any sync
//   MEDIAFLICK_VERIFY_UPGRADE_PHASE    update | back | forward
//   MEDIAFLICK_VERIFY_UPGRADE_SCHEMA   the schema this binary leaves library.db at
//   MEDIAFLICK_VERIFY_UPGRADE_LAUNCHED_AFTER  Unix time just before the session launched the app
//
// with --account SERVER, --seed and --seed-data of that profile. The session
// puts the cached token into the profile's own credentials row.
//
// Every expectation comes from before.json, which the real 0.2.0 (or the
// previous start) wrote: the user, the Jellyfin device, the JSON files and
// custom posters byte for byte (accounts.json without the page it last
// showed, which the app saves on every navigation; scripts/user-files.mjs),
// and each choice the 0.2.0 drive made in the
// UI. The checks: it starts signed in as that user on that device without a
// sign-in screen; no user file was rewritten by starting; library.db was not
// recreated and kept every row value for value up to the point a sync could
// refill it (pre-sync.json); the accent, Player, Application and Viewing
// choices, the Letterboxd profile, the collection with its poster bytes and
// the movie's subtitle choice all show; a sync then completes without a
// failure; and after a relaunch nothing migrates again. Every check is in
// upgrade-0-2-0.json and the run fails at the end listing each failed one.
import { readFileSync, readdirSync } from "node:fs"
import path from "node:path"
import { readDatabaseCopy, userFileDigests } from "../scripts/profile-state.mjs"

const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const SETTINGS_NAV = { role: "navigation", name: "Settings navigation" }

function required(name) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set; run scripts/e2e/upgrade-0-2-0.mjs instead of this drive alone`)
  return value
}

export default async function upgrade020(ctx) {
  const before = JSON.parse(readFileSync(required("MEDIAFLICK_VERIFY_UPGRADE_BEFORE"), "utf8"))
  const presync = JSON.parse(readFileSync(required("MEDIAFLICK_VERIFY_UPGRADE_PRESYNC"), "utf8"))
  const phase = required("MEDIAFLICK_VERIFY_UPGRADE_PHASE")
  const schema = Number(required("MEDIAFLICK_VERIFY_UPGRADE_SCHEMA"))
  const report = { phase, schema, checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }
  try {
    await steps(ctx, { before, presync, phase, schema, report, record })
  } finally {
    ctx.writeEvidence("upgrade-0-2-0.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function steps(ctx, { before, presync, phase, schema, report, record }) {
  const configDir = ctx.configDir
  const dataDir = path.join(configDir, "../../data/mediaflick-desktop")
  const launchedAfter = Number(required("MEDIAFLICK_VERIFY_UPGRADE_LAUNCHED_AFTER"))

  // Before the drive navigates anywhere.
  report.userFiles = userFileDigests(configDir)
  for (const name of new Set([...Object.keys(before.files), ...Object.keys(report.userFiles)])) {
    const now = report.userFiles[name] ?? null
    const expected = before.files[name] ?? null
    record(now === expected, `starting left ${name} as it was`, now === expected ? undefined : { before: expected, now })
  }

  for (const check of presync.checks) record(check.ok, `before any sync: ${check.message}`, check.detail)

  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  const status = (await ctx.api("/api/status")).body
  record(status?.authenticated === true && status.userName === before.credentials.user_name, `starts signed in as ${before.credentials.user_name}, with no sign-in screen`, status?.userName)
  const database = () => readDatabaseCopy(dataDir, (db) => ({
    userVersion: db.prepare("PRAGMA user_version").get().user_version,
    deviceId: db.prepare("SELECT device_id FROM credentials WHERE id = 1").get()?.device_id ?? null,
    meta: Object.fromEntries(db.prepare("SELECT key, value FROM meta").all().map((row) => [row.key, row.value])),
  }))
  const opened = database()
  report.database = { userVersion: opened.userVersion, deviceId: opened.deviceId }
  record(opened.deviceId === before.credentials.device_id, "the Jellyfin device is the one 0.2.0 signed in on", opened.deviceId)
  record(opened.userVersion === schema, `library.db is at schema ${schema}`, opened.userVersion)
  const log = ctx.appLog()
  record(!/recreating the .*library database/.test(log), "app.log shows no database recreation")
  const migrated = log.match(/library schema migrated \d+ → \d+/g) ?? []
  const expectMigration = phase !== "back" && before.userVersion < schema
  record(migrated.length === (expectMigration ? 1 : 0), expectMigration ? `app.log shows one migration from ${before.userVersion} to ${schema}` : "app.log shows no migration", migrated)

  const accent = await ctx.app.evaluate("document.documentElement.dataset.accent ?? null")
  record(accent === before.intent.accent, `the ${before.intent.accent} accent from accounts.json is applied`, accent)
  await ctx.screenshot("01-home")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf")
  const fullscreen = (await ctx.state({ role: "combobox", name: "Default fullscreen" }, { timeout: 30000 })).value
  record(fullscreen === before.intent.defaultFullscreen, `Player shows Default fullscreen ${before.intent.defaultFullscreen}`, fullscreen)
  await ctx.press({ role: "link", name: "Application", within: SETTINGS_NAV })
  const scrollbars = (await ctx.state({ role: "switch", name: "Show scrollbars" }, { timeout: 30000 })).checked
  record(scrollbars === before.intent.showScrollbars, `Application shows Show scrollbars ${before.intent.showScrollbars ? "on" : "off"}`, scrollbars)
  await ctx.screenshot("02-application")
  await ctx.press({ role: "link", name: "Viewing", within: SETTINGS_NAV })
  const hideWatched = (await ctx.state({ role: "switch", name: "Hide watched by default" }, { timeout: 30000 })).checked
  record(hideWatched === before.intent.hideWatched, `Viewing shows Hide watched by default ${before.intent.hideWatched ? "on" : "off"}`, hideWatched)
  await ctx.press({ role: "link", name: "Letterboxd", within: SETTINGS_NAV })
  const letterboxd = await ctx.until(async () => {
    const text = await ctx.app.evaluate(`document.querySelector("main")?.textContent ?? ""`)
    return before.intent.letterboxd.every((name) => text.includes(name)) ? before.intent.letterboxd : null
  }, "the Letterboxd profiles 0.2.0 saved", 30000).catch(() => null)
  record(letterboxd !== null, `the Letterboxd shelf lists ${before.intent.letterboxd.join(", ")}`)
  await ctx.screenshot("03-letterboxd")

  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name: "My Collections" })
  for (const collection of before.intent.collections) {
    const link = { role: "link", name: `Open ${collection.title}` }
    await ctx.find(link, { timeout: 30000 })
    record(true, `My Collections shows ${collection.title}`)
    if (!collection.posterSha256) continue
    const shown = await ctx.app.evaluate(`(async () => {
      const link = [...document.querySelectorAll("a")].find((node) => node.getAttribute("aria-label") === ${JSON.stringify(`Open ${collection.title}`)} || node.textContent.includes(${JSON.stringify(collection.title)}))
      const image = link?.querySelector("img")
      if (!image) return null
      const bytes = await (await fetch(image.src)).arrayBuffer()
      return [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((byte) => byte.toString(16).padStart(2, "0")).join("")
    })()`)
    record(shown === collection.posterSha256, `${collection.title} shows its custom poster, byte for byte`, shown)
  }
  await ctx.park()
  await ctx.screenshot("04-my-collections")

  for (const choice of before.intent.playback) {
    await ctx.app.route(`/item/${choice.itemId}`)
    await ctx.app.evaluate(`document.getElementById(${JSON.stringify(choice.select === "Subtitle track" ? "subtitle-track-select" : "audio-track-select")})?.scrollIntoView({ block: "center" })`)
    const shown = (await ctx.state({ role: "combobox", name: choice.select }, { timeout: 30000 })).value ?? ""
    record(shown.includes(choice.shows), `the movie shows its saved ${choice.select}: ${choice.shows}`, shown)
  }
  await ctx.screenshot("05-playback-choice")

  const synced = await ctx.until(async () => {
    const meta = database().meta
    return Number(meta["sync.completed_at"] ?? 0) >= launchedAfter && !(await ctx.api("/api/status")).body?.syncProgress?.active ? meta : null
  }, "a sync to complete after the launch", 300000).catch(() => null)
  record(synced !== null && synced["sync.latest_failure"] === "0", "a sync completes on this library.db without a failure", synced && { completedAt: synced["sync.completed_at"], latestFailure: synced["sync.latest_failure"], report: synced["sync.last_report"] })

  await ctx.relaunch()
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  const again = (await ctx.api("/api/status")).body
  record(again?.authenticated === true && again.userName === before.credentials.user_name, "after a relaunch it is still signed in", again?.userName)
  const relaunched = database()
  record(relaunched.userVersion === schema && relaunched.deviceId === before.credentials.device_id, "after a relaunch library.db keeps its schema and device", { userVersion: relaunched.userVersion, deviceId: relaunched.deviceId })
  const relaunchLog = ctx.appLog()
  record((relaunchLog.match(/library schema migrated/g) ?? []).length === migrated.length && !/recreating the .*library database/.test(relaunchLog), "the relaunch migrates and recreates nothing")
  await ctx.screenshot("06-after-relaunch")
  report.customArt = readdirSync(path.join(configDir, "custom-art")).sort()
}
