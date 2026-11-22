import { createHash } from "node:crypto"
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"
import { readDatabaseCopy, userFileDigests } from "../scripts/profile-state.mjs"

const REASON_LIMIT = 600
const SIDECARS = ["", "-wal", "-shm"]
const BACKUP = /^library-replaced-\d{4}-\d\d-\d\d-\d{6}Z\.db$/

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex")
const collapsed = (value) => value.replace(/\s+/g, " ").trim()
const escaped = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")

function databaseFiles(dir, name) {
  return Object.fromEntries(SIDECARS.filter((suffix) => existsSync(path.join(dir, `${name}${suffix}`))).map((suffix) => [suffix || "db", sha256(readFileSync(path.join(dir, `${name}${suffix}`)))]))
}

const post = (ctx, apiPath, body) => ctx.app.evaluate(`fetch(${JSON.stringify(apiPath)}, { method: "POST", headers: { "Content-Type": "application/json" }, body: ${JSON.stringify(JSON.stringify(body))} }).then(async (response) => ({ status: response.status, body: await response.json().catch(() => null) }))`)

function userFiles(configDir) {
  const reader = path.join(configDir, "reader.json")
  return { ...userFileDigests(configDir), ...(existsSync(reader) ? { "reader.json": sha256(readFileSync(reader)) } : {}) }
}

function required(name) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set; run scripts/e2e/upgrade-0-2-0.mjs instead of this drive alone`)
  return value
}

export default async function libraryRefused(ctx) {
  const expectedSha = required("MEDIAFLICK_VERIFY_REFUSED_SHA256")
  const expectedSchema = Number(required("MEDIAFLICK_VERIFY_REFUSED_SCHEMA"))
  const expectedReason = JSON.parse(required("MEDIAFLICK_VERIFY_REFUSED_REASON"))
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }
  try {
    const opens = () => (ctx.appLog().match(/could not open .*library\.db/g) ?? []).length
    await ctx.until(() => opens() > 0, "app.log to report the refused library.db", 30000)
    const answers = []
    for (let index = 0; index < 6; index += 1) answers.push(await ctx.api("/api/status"))
    report.answers = answers.map((answer) => ({ status: answer.status, body: answer.body }))
    record(answers.every((answer) => answer.status === 503), "every /api/status answers 503 while the services are down", answers.map((answer) => answer.status))
    const logged = ctx.appLog().match(/could not open [^\r\n]*?library\.db: [\s\S]*?(?=\n\d{4}-\d\d-\d\dT|\n?$)/)?.[0] ?? null
    const reason = answers[0].body?.error ?? ""
    const matches = (patterns, text) => patterns.every((pattern) => new RegExp(pattern).test(text))
    record(matches(expectedReason.patterns, reason), `the 503 says ${expectedReason.says}`, answers[0].body)
    record(reason.length <= REASON_LIMIT, `the reason is at most ${REASON_LIMIT} characters`, reason.length)
    if (expectedReason.leavesOut) {
      record(!expectedReason.leavesOut.patterns.some((pattern) => new RegExp(pattern).test(reason)), `the reason leaves out ${expectedReason.leavesOut.says}`)
    }
    if (expectedReason.logKeeps) {
      record(matches(expectedReason.logKeeps.patterns, ctx.appLog()), `app.log keeps ${expectedReason.logKeeps.says}`)
    }
    record(answers.every((answer) => answer.body?.startupFailed === true), "every 503 is marked as a failed startup", answers.map((answer) => answer.body?.startupFailed))
    record(logged !== null && reason === logged, "the 503 carries the reason app.log gave", logged)
    await ctx.until(() => ctx.windowShown(), "the main window to be shown", 30000)
    record(true, "the main window is shown")
    const screen = await ctx.until(
      `document.querySelector('[role="heading"][aria-level="1"]')?.textContent === "MediaFlick could not start"`,
      "the startup error screen",
      30000,
    ).then(() => true, () => false)
    await ctx.screenshot("01-refused")
    record(screen, "the startup error screen is shown")
    const text = await ctx.app.evaluate("document.body.innerText")
    record(logged !== null && collapsed(text).includes(collapsed(logged)), "the screen shows the reason app.log gave", text)
    record(!(await ctx.exists({ role: "button", name: "Sign in" })) && !text.includes("Welcome to MediaFlick"), "the sign-in screen is not shown")
    record(opens() === 1, "the open was tried once, not again for each request", opens())
    const file = path.join(ctx.configDir, "../../data/mediaflick-desktop/library.db")
    const sha = createHash("sha256").update(readFileSync(file)).digest("hex")
    record(sha === expectedSha, "library.db keeps its bytes", sha)
    const db = new DatabaseSync(file, { readOnly: true })
    let deviceId
    try {
      const version = db.prepare("PRAGMA user_version").get().user_version
      record(version === expectedSchema, `library.db keeps schema ${expectedSchema}`, version)
      deviceId = db.prepare("SELECT device_id FROM credentials WHERE id = 1").get()?.device_id ?? null
    } finally {
      db.close()
    }
    if (required("MEDIAFLICK_VERIFY_REFUSED_KIND") === "inUse") await inUse(ctx, record, { dataDir: path.dirname(file) })
    else await recover(ctx, record, report, { opens, deviceId, dataDir: path.dirname(file) })
  } finally {
    ctx.writeEvidence("library-refused.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function recover(ctx, record, report, { opens, deviceId, dataDir }) {
  const kind = required("MEDIAFLICK_VERIFY_REFUSED_KIND")
  const schema = Number(required("MEDIAFLICK_VERIFY_SCHEMA"))
  const openedLog = required("MEDIAFLICK_VERIFY_OPENED_LOG")
  const recreate = { role: "button", name: "Recreate library" }
  const openFolder = { role: "button", name: "Open data folder" }
  const offered = async (target) => ctx.find(target, { timeout: 15000 }).then(() => true, () => false)
  const offersRecreate = await offered(recreate)
  const offersFolder = await offered(openFolder)
  record(offersRecreate, "the startup error screen offers Recreate library")
  record(offersFolder, "the startup error screen offers Open data folder")
  await ctx.screenshot("02-recovery")
  const files = userFiles(ctx.configDir)
  report.userFiles = files

  const pressed = (target) => ctx.press(target).then(() => true, (error) => {
    ctx.step(`could not press ${target.name}: ${error.message.split("\n")[0]}`)
    return false
  })
  const dialog = { role: "alertdialog", name: "Recreate the library?" }
  const openDialog = async () => (await pressed(recreate)) && ctx.find(dialog).then(() => true, () => false)
  const SHORT = { width: 1280, height: 400 }
  const fits = () => ctx.app.evaluate(`(() => {
    const dialog = document.querySelector('[role="alertdialog"]')
    if (!dialog) return null
    const box = dialog.getBoundingClientRect()
    return { top: box.top, bottom: box.bottom, viewport: innerHeight, scrolls: dialog.scrollHeight > dialog.clientHeight }
  })()`)

  if (offersFolder) await pressed(openFolder)
  const opened = offersFolder ? await ctx.until(() => existsSync(openedLog) && readFileSync(openedLog, "utf8").trim(), "xdg-open to be called", 15000).catch(() => null) : null
  record(opened === path.resolve(dataDir), "Open data folder hands the folder holding library.db to xdg-open", opened)
  record(ctx.appLog().includes(`opening folder in the file manager folder=${path.resolve(dataDir)}`), "app.log names the folder it opened")

  const beforeCancel = databaseFiles(dataDir, "library.db")
  await ctx.app.send("Emulation.setDeviceMetricsOverride", { ...SHORT, deviceScaleFactor: 1, mobile: false })
  const shown = await openDialog()
  const text = collapsed(await ctx.app.evaluate(`document.querySelector('[role="alertdialog"]')?.innerText ?? ""`))
  report.dialog = text
  const fit = await fits()
  record(fit !== null && fit.top >= 0 && fit.bottom <= fit.viewport && fit.scrolls, `in a ${SHORT.width}x${SHORT.height} window the dialog fits on screen and scrolls`, fit)
  await ctx.screenshot("03-recreate-dialog")
  const says = [
    ["the user signs in again and Jellyfin sees a new device", /You sign in again, and Jellyfin lists MediaFlick as a new device\./],
    ["the library syncs again, and watch state and favorites are kept on Jellyfin", /The local copy of your library, which syncs again from Jellyfin\. Watched state, resume points and favorites are kept on Jellyfin/],
    ["ratings and collection titles refill through the Companion's MDBList and TMDB, and a manually refreshed collection stays empty", /Cached ratings and the titles in your collections\. Refilling them spends MDBList and TMDB requests through the Companion, and a collection that refreshes manually stays empty until you refresh it\./],
    ["settings, Letterboxd profiles, collections, posters, track choices and reading positions are kept", /You keep your settings, Letterboxd profiles, collections and their posters, audio and subtitle choices, and reading positions\./],
    ["nothing is deleted, and the old files go next to it under a dated name", new RegExp(`Nothing is deleted: library\\.db and its -wal and -shm files are renamed with the date and time, to a name like library-replaced-\\d{4}-\\d\\d-\\d\\d-\\d{6}Z\\.db, in ${escaped(path.resolve(dataDir))}\\.`)],
  ]
  for (const [what, pattern] of says) record(pattern.test(text), `the dialog says ${what}`)
  const newer = /written by a newer MediaFlick\. Recreating replaces that version's library too: installed again, it opens the new one\. To keep using its library instead, cancel and install that newer MediaFlick again\./.test(text)
  record(kind === "newerSchema" ? newer : !/newer MediaFlick/.test(text), kind === "newerSchema" ? "the dialog says the newer MediaFlick's library is replaced too, and that installing it again is the alternative" : "the dialog does not speak of a newer MediaFlick")
  const cancel = { role: "button", name: "Cancel", within: dialog }
  if (shown) await ctx.app.evaluate(`document.querySelector('[role="alertdialog"]')?.scrollTo({ top: 1e6 })`)
  if (shown) await ctx.screenshot("03-recreate-dialog-end")
  const cancelled = shown && await pressed(cancel)
  record(cancelled, `in a ${SHORT.width}x${SHORT.height} window the dialog's Cancel is reachable`)
  if (cancelled) await ctx.until(async () => !(await ctx.exists(dialog)), "the dialog to close", 10000)
  await ctx.sleep(2000)
  const afterCancel = databaseFiles(dataDir, "library.db")
  record(JSON.stringify(afterCancel) === JSON.stringify(beforeCancel), "Cancel leaves library.db and its sidecars byte for byte as they were", { before: beforeCancel, after: afterCancel })
  record(!readdirSync(dataDir).some((name) => name.startsWith("library-replaced-")), "Cancel moves nothing aside", readdirSync(dataDir))
  record((await ctx.api("/api/status")).status === 503 && opens() === 1, "after Cancel the services are still down, and the open was still tried only once", opens())

  const moved = databaseFiles(dataDir, "library.db")
  report.moved = moved
  const recreatedAfter = Math.floor(Date.now() / 1000)
  const confirmed = (await openDialog()) && await pressed({ role: "button", name: "Recreate library", within: dialog })
  record(confirmed, `in a ${SHORT.width}x${SHORT.height} window the dialog's Recreate library is reachable`)
  await ctx.app.send("Emulation.clearDeviceMetricsOverride")
  const up = await ctx.until(async () => (await ctx.api("/api/status")).status === 200, "the services to open", 30000).then(() => true, () => false)
  record(up, "after Recreate library /api/status answers 200")
  const player = await post(ctx, "/api/player/command", { command: "set-mute", mute: false })
  record(player.status === 200, "the window's player is attached to the services that opened", player)
  const ready = await post(ctx, "/api/shell/window/ready", {})
  record(ready.status === 200 && ready.body?.queued === true, "the window serves the shell requests of the services that opened", ready)
  const backups = readdirSync(dataDir).filter((name) => name.startsWith("library-replaced-") && name.endsWith(".db"))
  report.backups = readdirSync(dataDir).filter((name) => name.startsWith("library-replaced-"))
  record(backups.length === 1 && BACKUP.test(backups[0]), "one library-replaced-<UTC time>.db is next to library.db", report.backups)
  const backup = backups.length === 1 ? databaseFiles(dataDir, backups[0]) : {}
  record(Object.keys(moved).length > 0 && JSON.stringify(backup) === JSON.stringify(moved), `library.db and its sidecars (${Object.keys(moved).join(", ")}) moved there byte for byte`, { moved, backup })
  const stamp = backups[0]?.match(/(\d{4})-(\d\d)-(\d\d)-(\d\d)(\d\d)(\d\d)Z/)
  const stampedAt = stamp ? Date.UTC(stamp[1], stamp[2] - 1, stamp[3], stamp[4], stamp[5], stamp[6]) / 1000 : NaN
  record(stampedAt >= recreatedAfter && stampedAt <= Math.floor(Date.now() / 1000), "the backup's name carries the UTC second it moved", { name: backups[0], recreatedAfter })
  const log = ctx.appLog()
  record(opens() === 1 && (log.match(/library database ready/g) ?? []).length === 1, "app.log has the one refused open and then one open of the new library.db", { refused: opens(), ready: (log.match(/library database ready/g) ?? []).length })
  record(log.includes("Recreate library: moved the refused"), "app.log says where Recreate library moved the old file")
  const fresh = await readDatabaseCopy(dataDir, (db) => ({
    userVersion: db.prepare("PRAGMA user_version").get().user_version,
    items: db.prepare("SELECT count(*) AS count FROM items").get().count,
    deviceId: db.prepare("SELECT device_id FROM credentials WHERE id = 1").get()?.device_id ?? null,
  }))
  record(fresh.userVersion === schema && fresh.items === 0, `the new library.db is empty, at schema ${schema}`, fresh)
  const now = userFiles(ctx.configDir)
  record(JSON.stringify(now) === JSON.stringify(files), "the JSON files, reader.json and custom-art/ are as they were", { before: files, after: now })

  const signIn = await ctx.until(async () => (await ctx.exists({ role: "button", name: "Use Quick Connect" })) && !(await ctx.exists({ role: "heading", name: "MediaFlick could not start" })), "the sign-in screen", 30000).then(() => true, () => false)
  await ctx.screenshot("04-sign-in")
  record(signIn, "the same window leaves the startup error screen for the sign-in screen")
  const signedIn = signIn && await ctx.signIn({}).then(() => true, (error) => {
    ctx.step(`could not sign in: ${error.message.split("\n")[0]}`)
    return false
  })
  if (signedIn) ctx.revokeAppSessionAfterRun()
  const status = (await ctx.api("/api/status")).body
  record(signedIn && status?.authenticated === true && status.deviceId !== deviceId, "Quick Connect signs in as a new device", { userName: status?.userName, deviceId: status?.deviceId, previous: deviceId })
  const synced = signedIn ? await ctx.until(async () => {
    const meta = await readDatabaseCopy(dataDir, (db) => Object.fromEntries(db.prepare("SELECT key, value FROM meta").all().map((row) => [row.key, row.value])))
    const current = (await ctx.api("/api/status")).body
    return Number(meta["sync.completed_at"] ?? 0) >= recreatedAfter && !current?.syncProgress?.active ? { meta, library: current.library } : null
  }, "a sync to fill the new library.db", 300000).catch(() => null) : null
  record(synced !== null && synced.meta["sync.latest_failure"] === "0" && synced.library.total > 0, "a sync fills the new library.db from the server without a failure", synced && { completedAt: synced.meta["sync.completed_at"], latestFailure: synced.meta["sync.latest_failure"], library: synced.library })
  if (!signedIn) return
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 30000 })
  await ctx.screenshot("05-home-after-resync")
}

async function inUse(ctx, record, { dataDir }) {
  try {
    const recovery = await ctx.api("/api/recovery")
    record(recovery.status === 200 && recovery.body?.library === null, "/api/recovery offers nothing to recreate", recovery)
    await ctx.sleep(2000)
    const offered = (await ctx.exists({ role: "button", name: "Recreate library" })) || (await ctx.exists({ role: "button", name: "Open data folder" }))
    await ctx.screenshot("02-in-use")
    record(!offered, "the startup error screen offers no Recreate library while another program holds library.db")
    const before = databaseFiles(dataDir, "library.db")
    const asked = await post(ctx, "/api/recovery/library/recreate", {})
    record(asked.status === 409, "a Recreate library asked for directly is refused", asked)
    const after = databaseFiles(dataDir, "library.db")
    record(JSON.stringify(after) === JSON.stringify(before) && !readdirSync(dataDir).some((name) => name.startsWith("library-replaced-")), "library.db and its sidecars stay where they were, byte for byte", { before, after, files: readdirSync(dataDir) })
  } finally {
    writeFileSync(required("MEDIAFLICK_VERIFY_HOLD_RELEASE"), "release\n")
  }
}
