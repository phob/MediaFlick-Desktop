// A storage failure in an optional sync step reaches the user as a storage
// failure. The app signs in through a loopback proxy that holds the first
// /UserViews answer; while it is held the drive takes SQLite's write lock on
// the profile's library.db from this process, as another program holding the
// file would, then lets the answer through. Storing the library list waits out
// the app's 10 s busy timeout and fails. The retry screen must name local
// storage, and after the lock is gone Retry now must load Home with the
// library list stored. Read-only against the demo server.
//   just verify storage-failure
//
// Before the fix the failure was logged as "could not sync the library list"
// and the cycle went on as if the list were merely not loaded yet. The drive
// releases the lock as soon as it sees that line, so the old build shows
// Home without a library list instead of the storage screen, and fails.
import fs from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"

const DEMO = "https://demo.jellyfin.org/stable"
const STORAGE_TITLE = "MediaFlick couldn't save your library"
const REASON = { role: "heading", name: STORAGE_TITLE }
const RETRY = { role: "button", name: "Retry now" }
const SWALLOWED = "could not sync the library list"
const USER_FILES = ["settings.json", "accounts.json"]

const status = async (ctx) => (await ctx.api("/api/status")).body
const pageText = (ctx) => ctx.app.evaluate("document.body.innerText")

function userFiles(ctx) {
  return Object.fromEntries(USER_FILES.map((name) => {
    const file = ctx.configFile(name)
    const bytes = fs.existsSync(file) ? fs.readFileSync(file) : null
    let parses = false
    if (bytes) {
      try {
        JSON.parse(bytes.toString("utf8"))
        parses = true
      } catch {
        parses = false
      }
    }
    return [name, { exists: bytes !== null, bytes: bytes?.length ?? 0, parses }]
  }))
}

// BEGIN IMMEDIATE takes the WAL write lock and fails at once while the app is
// itself writing, so try until the app is between writes.
async function holdWriteLock(ctx, file) {
  const database = new DatabaseSync(file)
  await ctx.until(() => {
    try {
      database.exec("BEGIN IMMEDIATE")
      return true
    } catch (error) {
      ctx.step(`write lock not taken yet: ${error.message}`)
      return false
    }
  }, "the write lock on library.db", 15000)
  let held = true
  return {
    release() {
      if (!held) return
      held = false
      database.exec("ROLLBACK")
      database.close()
      ctx.step("released the write lock on library.db")
    },
  }
}

export default async function storageFailure(ctx) {
  const report = { checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.check(condition, message)
  }
  let openList
  const listHeld = new Promise((resolve) => {
    openList = resolve
  })
  let heldLists = 0
  const proxy = await ctx.proxy({
    upstream: DEMO,
    answer: (url) => {
      if (url.pathname !== "/UserViews" || heldLists > 0) return null
      heldLists += 1
      return listHeld.then(() => null)
    },
  })

  await ctx.find({ role: "button", name: "Sign in" }, { timeout: 30000 })
  await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
  await ctx.fill({ role: "textbox", name: "Username" }, "demo")
  await ctx.press({ role: "button", name: "Sign in" })
  await ctx.until(async () => (await status(ctx))?.authenticated === true, "the session to be signed in", 30000)
  await ctx.until(() => proxy.stats.held.some((entry) => entry.path === "/UserViews"), "the sync to ask for the library list", 30000)
  report.filesBefore = userFiles(ctx)

  const databaseFile = path.join(ctx.dataDir, "library.db")
  const lock = await holdWriteLock(ctx, databaseFile)
  try {
    openList()
    ctx.step("let the library list through while library.db is locked")
    const outcome = await ctx.until(async () => {
      if (ctx.appLog().includes(SWALLOWED)) return "swallowed"
      return (await ctx.exists(REASON)) ? "storage" : null
    }, "the failed library list write to reach the user or the log", 60000)
    report.outcome = outcome
    if (outcome === "swallowed") {
      lock.release()
      await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
      report.swallowedStatus = await status(ctx)
      report.swallowedLibraries = (await ctx.api("/api/libraries")).body
      await ctx.screenshot("01-swallowed")
      ctx.writeEvidence("storage-failure.json", report)
      throw new Error("the failed library list write was logged and skipped instead of failing the sync")
    }

    const retrying = await status(ctx)
    report.retryingStatus = retrying
    expect(retrying.bootstrap?.state === "retrying" && retrying.bootstrap.reason === "storage", "/api/status reports bootstrap retrying, storage")
    const text = await pageText(ctx)
    expect(text.includes("Check that the disk has free space"), "the retry screen gives the storage advice")
    expect(!/database is locked|sqlite|rusqlite|os error/i.test(text), "no raw storage error text on screen")
    expect(!ctx.appLog().includes(SWALLOWED), "the app never logged the failure as a skipped step")
    await ctx.find(RETRY)
    await ctx.snapshot("01-storage-retry-screen")
    await ctx.screenshot("01-storage-retry-screen")
  } finally {
    lock.release()
  }

  await ctx.until(async () => (await ctx.state(RETRY)).disabled !== true, "Retry now to be available")
  await ctx.press(RETRY)
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  const ready = await ctx.until(async () => {
    const body = await status(ctx)
    return body?.bootstrap?.state === "ready" ? body : null
  }, "bootstrap ready", 60000)
  report.readyStatus = ready
  const libraries = (await ctx.api("/api/libraries")).body
  report.libraries = libraries
  const listedIds = Array.isArray(libraries?.libraries) ? libraries.libraries.map((library) => library.id) : []
  const views = await ctx.jellyfin(`/UserViews?userId=${encodeURIComponent(ready.userId)}`)
  report.serverViews = views.Items.map((view) => view.Id)
  expect(
    listedIds.length > 0 && JSON.stringify(listedIds) === JSON.stringify(report.serverViews),
    `after the retry the stored library list equals /UserViews (${listedIds.length} libraries)`,
  )
  report.filesAfter = userFiles(ctx)
  for (const name of USER_FILES) {
    expect(report.filesAfter[name].exists && report.filesAfter[name].parses, `${name} is intact`)
  }
  ctx.step(`proxy forwarded ${proxy.stats.forwarded} requests and held ${proxy.stats.held.length}`)
  await ctx.snapshot("02-home")
  await ctx.screenshot("02-home")
  ctx.writeEvidence("storage-failure.json", report)
}
