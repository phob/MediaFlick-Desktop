// Memory while reading fixed pages (`--account`, Linux): turns 250 pages
// of the 85-page PDF (bouncing between its ends, so pages are read again)
// and 540 of the 271-page CBZ (to its end and back), and every 10 turns records the renderer processes' resident memory (from
// /proc), the JS heap, the document and frame counts, the page's live blob URLs, workers and iframes, and how many PDF page load
// results are still reachable after garbage collection (checked: only the shown page's), and the page's unnamed native functions
// (checked: no growth after the first 10 turns; every reaction left on a pending promise keeps two). A curve that keeps rising with pages read,
// rather than levelling off, means pages are kept after they leave the
// screen. Then it opens and closes each book three more times: what the reader
// leaves alive after it closed must not grow with opens.
//
// Then the races, each checked: the PDF is opened and closed five times while
// pdf.js waits on a range read of the file, which must leave no worker, and each book takes
// 40 rapid double presses of ArrowRight (the second before the first page has
// loaded) and one held ArrowLeft, after which only the shown page may hold blob
// URLs. Last, the CBZ turns ahead until a page waits on a held read of the
// file, queues a turn back behind it and closes: no blob URL or iframe may
// outlive that. MEDIAFLICK_VERIFY_RESIDENCY=races runs only the races. Writes
// books-residency.json.
import { readdirSync, readFileSync } from "node:fs"

const BOOKS = [
  { key: "pdf", name: "The Constitution of the United States - US Congress", pages: 85, turns: 250 },
  { key: "cbz", name: "Shen of the Sea - Arthur Bowie Chrisman", pages: 271, turns: 540 },
]
const REOPENS = 3
const CLOSES_WHILE_OPENING = 5
const DOUBLE_PRESSES = 40
const HELD_REPEATS = 20
/** A shown page's document and image, twice over for a page whose image has not loaded yet. */
const MAX_SHOWN_BLOBS = 4
/**
 * Counts the page's live blob URLs and workers: the reader's engines run in
 * the page itself. Armed with `closeAtRange`, the next read of a book's file
 * past its first block (pdf.js asking for a range once its worker runs)
 * leaves the reader while that read is still in flight.
 */
const INSTRUMENT = `(() => {
  if (window.__residency) return
  const blobs = new Map()
  const workers = new Set()
  const create = URL.createObjectURL.bind(URL)
  const revoke = URL.revokeObjectURL.bind(URL)
  URL.createObjectURL = (object) => { const url = create(object); blobs.set(url, object.type || "untyped"); return url }
  URL.revokeObjectURL = (url) => { blobs.delete(url); revoke(url) }
  const NativeWorker = window.Worker
  window.Worker = class extends NativeWorker {
    constructor(...args) { super(...args); workers.add(this) }
    terminate() { workers.delete(this); super.terminate() }
  }
  const iframes = []
  const createElement = Document.prototype.createElement
  Document.prototype.createElement = function (name, options) {
    const element = createElement.call(this, name, options)
    if (String(name).toLowerCase() === "iframe") iframes.push(new WeakRef(element))
    return element
  }
  const residency = { blobs, workers, iframes, closeAtRange: false, closedAtBytes: [], holdRange: false, heldAtBytes: [] }
  const nativeFetch = window.fetch
  window.fetch = function (input, init) {
    const response = nativeFetch.call(this, input, init)
    const url = new URL(input instanceof Request ? input.url : String(input), location.href)
    const start = Number(url.searchParams.get("start"))
    if (residency.closeAtRange && url.pathname.endsWith("/book/file") && start > 0) {
      residency.closeAtRange = false
      residency.closedAtBytes.push(start)
      history.back()
    }
    if (residency.holdRange && url.pathname.endsWith("/book/file") && start > 0 && init?.signal) {
      residency.holdRange = false
      residency.heldAtBytes.push(start)
      const aborted = new Promise((resolve) => init.signal.addEventListener("abort", resolve, { once: true }))
      return Promise.all([response, aborted]).then(([answer]) => answer)
    }
    return response
  }
  window.__residency = residency
})()`
const LIVE = `({
  blobUrls: window.__residency.blobs.size,
  blobTypes: [...window.__residency.blobs.values()].reduce((counts, type) => ({ ...counts, [type]: (counts[type] ?? 0) + 1 }), {}),
  workers: window.__residency.workers.size,
  iframes: window.__residency.iframes.filter((ref) => ref.deref()).length,
})`
/** How many heap objects with `prototype` (an expression) pass `filter` (a function's source). */
async function countObjects(ctx, prototype, filter) {
  const group = "residency-count"
  const { result } = await ctx.app.send("Runtime.evaluate", { expression: prototype, objectGroup: group })
  const { objects } = await ctx.app.send("Runtime.queryObjects", { prototypeObjectId: result.objectId, objectGroup: group })
  const counted = await ctx.app.send("Runtime.callFunctionOn", { objectId: objects.objectId, functionDeclaration: `function () { return this.filter(${filter}).length }`, returnByValue: true })
  await ctx.app.send("Runtime.releaseObjectGroup", { objectGroup: group })
  if (counted.exceptionDetails) throw new Error(`counting ${prototype} objects failed: ${counted.exceptionDetails.exception?.description ?? counted.exceptionDetails.text}`)
  return counted.result.value
}
/**
 * PDF page load results (`{ src, onZoom, images }`), and the page's unnamed
 * native functions: a promise's resolve and reject functions are those, and
 * every reaction left on a promise that stays pending keeps a pair.
 */
const heapCounts = async (ctx) => ({
  pageResults: await countObjects(ctx, "Object.prototype", `(object) => Object.hasOwn(object, "src") && Object.hasOwn(object, "onZoom") && Object.hasOwn(object, "images")`),
  nativeUnnamedFunctions: await countObjects(ctx, "Function.prototype", `(fn) => { try { return fn.name === "" && Function.prototype.toString.call(fn).includes("[native code]") } catch { return false } }`),
})
/** The shown PDF page's load result. */
const MAX_SHOWN_PAGE_RESULTS = 1
/** Noise in the page's count of unnamed native functions (one sample rose 62 above its neighbours); a leak per page read grew it by 486. */
const MAX_FUNCTION_GROWTH = 150
const PAGES = 100
const SAMPLE_EVERY = 10

function read(path) {
  try {
    return readFileSync(path, "utf8")
  } catch {
    return null
  }
}

/**
 * Resident memory of the app's child processes (renderers, GPU, utilities),
 * found as descendants of the browser process, whose environment names this
 * run's profile. A sandboxed renderer's /proc entry may refuse reads; those
 * are counted in `unreadable` instead of being skipped silently.
 */
function childMemory(profile) {
  const processes = readdirSync("/proc")
    .filter((name) => /^\d+$/.test(name))
    .map((pid) => ({ pid, stat: read(`/proc/${pid}/stat`) }))
    .filter(({ stat }) => stat)
    .map(({ pid, stat }) => ({ pid, parent: stat.slice(stat.lastIndexOf(")") + 2).split(" ")[1] }))
  const browser = processes.find(({ pid }) => {
    const command = read(`/proc/${pid}/cmdline`) ?? ""
    return command.includes("mediaflick-desktop") && !command.includes("--type=") && read(`/proc/${pid}/environ`)?.includes(profile)
  })
  const children = new Set()
  let grew = true
  while (grew) {
    grew = false
    for (const { pid, parent } of processes) {
      if (!children.has(pid) && (parent === browser?.pid || children.has(parent))) {
        children.add(pid)
        grew = true
      }
    }
  }
  let rssKiB = 0
  let unreadable = 0
  const byType = {}
  for (const pid of children) {
    const status = read(`/proc/${pid}/status`)
    const rss = status && /VmRSS:\s+(\d+) kB/.exec(status)
    if (!rss) {
      unreadable += 1
      continue
    }
    const type = /--type=([\w-]+)/.exec(read(`/proc/${pid}/cmdline`) ?? "")?.[1] ?? "other"
    rssKiB += Number(rss[1])
    byType[type] = (byType[type] ?? 0) + Number(rss[1])
  }
  return { browser: browser?.pid ?? null, children: children.size, rssKiB, byType, unreadable }
}

async function sample(ctx, profile, turns) {
  await ctx.app.send("HeapProfiler.collectGarbage")
  const { metrics } = await ctx.app.send("Performance.getMetrics")
  const metric = (name) => metrics.find((entry) => entry.name === name)?.value
  return {
    turns,
    ...childMemory(profile),
    jsHeapUsedKiB: Math.round(metric("JSHeapUsedSize") / 1024),
    documents: metric("Documents"),
    frames: metric("Frames"),
    nodes: metric("Nodes"),
    ...(await heapCounts(ctx)),
    ...(await ctx.app.evaluate(LIVE)),
  }
}

export default async function (ctx) {
  await ctx.signIn({})
  await ctx.app.send("Performance.enable")
  await ctx.app.evaluate(INSTRUMENT)
  const profile = ctx.configFile("").split("/config/")[0]
  const answer = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Book`)
  const report = { profile: profile.split("/").pop(), books: {}, races: {}, checks: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, detail })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message} ${JSON.stringify(detail)}`)
  }
  const books = BOOKS.map((book) => {
    const id = answer.Items.find((item) => item.Name === book.name)?.Id
    ctx.check(Boolean(id), `the Books library has "${book.name}"`)
    ctx.restoreItemAfterRun(id)
    return { ...book, id }
  })
  try {
    if (process.env.MEDIAFLICK_VERIFY_RESIDENCY !== "races") for (const book of books) await traverse(ctx, profile, book, report, check)
    await closeWhileOpening(ctx, books[0], report, check)
    await closeWithTurnQueued(ctx, books[1], report, check)
    for (const book of books) await race(ctx, book, report, check)
  } finally {
    ctx.writeEvidence("books-residency.json", JSON.stringify(report, null, 2))
  }
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} residency checks failed: ${failed.map((entry) => entry.message).join("; ")}`)
}

const footer = `document.querySelector("[data-reader-footer]").innerText`
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function open(ctx, book, label) {
  await ctx.app.evaluate(`history.pushState(null, "", "/read/${book.id}?from=start"); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(`document.querySelector('[data-reader-book="${book.id}"]')?.dataset.readerPhase === "ready"`, `${book.key}: the reader opened${label}`, 120000)
}

async function close(ctx, book, label) {
  await ctx.key("Escape", { keyCode: 27 })
  await ctx.until(`!document.querySelector('[data-reader-book="${book.id}"]')`, `${book.key}: the reader closed${label}`, 20000)
  await ctx.app.send("HeapProfiler.collectGarbage")
  return ctx.app.evaluate(LIVE)
}

/** What the page still holds 2 s after the reader left, once garbage is collected. */
async function settledLive(ctx) {
  await pause(2000)
  await ctx.app.send("HeapProfiler.collectGarbage")
  return ctx.app.evaluate(LIVE)
}

async function closeWhileOpening(ctx, book, report, check) {
  const before = await ctx.app.evaluate(LIVE)
  const rounds = []
  for (let round = 1; round <= CLOSES_WHILE_OPENING; round += 1) {
    await ctx.app.evaluate(`window.__residency.closeAtRange = true; history.pushState(null, "", "/read/${book.id}?from=start"); dispatchEvent(new PopStateEvent("popstate"))`)
    await ctx.until(`window.__residency.closedAtBytes.length === ${round} && !document.querySelector('[data-reader-book="${book.id}"]')`, `${book.key}: the reader left during a range read (${round})`, 120000)
    rounds.push({ closedAtByte: await ctx.app.evaluate(`window.__residency.closedAtBytes[${round - 1}]`), ...(await settledLive(ctx)) })
  }
  report.races.closeWhileOpening = { book: book.key, before, rounds }
  check(rounds.every((entry) => entry.workers === before.workers), `${book.key}: no worker outlives a reader closed during its range read`, { before: before.workers, after: rounds.map((entry) => entry.workers) })
  check(rounds.every((entry) => entry.blobUrls === before.blobUrls), `${book.key}: no blob URL outlives a reader closed during its range read`, { before: before.blobUrls, after: rounds.map((entry) => entry.blobUrls) })
}

/**
 * Turns ahead until a turn's page needs a read of the file, holds that read,
 * queues a turn back behind it, and leaves the reader: the held read fails
 * when the reader leaves, and the queued turn must not load a page then.
 */
async function closeWithTurnQueued(ctx, book, report, check) {
  const before = await settledLive(ctx)
  await open(ctx, book, " to leave with a turn queued")
  await ctx.app.evaluate(`window.__residency.holdRange = true`)
  let turns = 0
  while (await ctx.app.evaluate(`window.__residency.holdRange`)) {
    const shown = await ctx.app.evaluate(footer)
    await ctx.key("ArrowRight", { keyCode: 39 })
    turns += 1
    await ctx.until(async () => !(await ctx.app.evaluate(`window.__residency.holdRange`)) || (await ctx.app.evaluate(footer)) !== shown, `${book.key}: turn ${turns} to show or to wait on a read`, 20000, { quiet: true })
  }
  await ctx.key("ArrowLeft", { keyCode: 37 })
  const queued = await ctx.app.evaluate(LIVE)
  await close(ctx, book, " with a turn queued")
  const after = await settledLive(ctx)
  report.races.closeWithTurnQueued = { book: book.key, turns, heldAtByte: await ctx.app.evaluate(`window.__residency.heldAtBytes.at(-1)`), before, queued, after }
  check(after.blobUrls === before.blobUrls && after.iframes <= before.iframes, `${book.key}: a turn queued behind a pending page leaves nothing once the reader left`, { before, after })
}

/**
 * Two presses back to back, so the second reaches the reader while the
 * first page is still loading, then one key held for HELD_REPEATS repeats.
 */
async function race(ctx, book, report, check) {
  const before = await ctx.app.evaluate(LIVE)
  await open(ctx, book, " for the races")
  const opened = await ctx.app.evaluate(LIVE)
  const samples = []
  const roundMs = []
  let page = 0
  for (let round = 1; round <= DOUBLE_PRESSES; round += 1) {
    const started = Date.now()
    await ctx.key("ArrowRight", { keyCode: 39 })
    await ctx.key("ArrowRight", { keyCode: 39 })
    page += 2
    await ctx.until(async () => (await ctx.app.evaluate(footer)).includes(`Page ${page + 1} of`), `${book.key}: double press ${round} to page ${page + 1}`, 20000, { quiet: true })
    roundMs.push(Date.now() - started)
    if (round % SAMPLE_EVERY === 0) samples.push({ round, page: page + 1, ...(await ctx.app.evaluate(LIVE)) })
  }
  const afterPresses = await settledLive(ctx)
  const held = { key: "ArrowLeft", code: "ArrowLeft", windowsVirtualKeyCode: 37 }
  for (let repeat = 0; repeat < HELD_REPEATS; repeat += 1) {
    await ctx.app.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...held, autoRepeat: repeat > 0 })
    await pause(33)
  }
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyUp", ...held })
  const released = Date.now()
  page -= HELD_REPEATS
  await ctx.until(async () => (await ctx.app.evaluate(footer)).includes(`Page ${page + 1} of`), `${book.key}: the held key to land on page ${page + 1}`, 20000)
  const heldSettledMs = Date.now() - released
  const afterHeld = await settledLive(ctx)
  const closed = await close(ctx, book, " after the races")
  const sorted = [...roundMs].sort((a, b) => a - b)
  const result = {
    before, opened, samples, afterPresses, afterHeld, closed, heldSettledMs,
    doublePressMs: { median: sorted[Math.floor(sorted.length / 2)], max: sorted[sorted.length - 1] },
  }
  report.races[book.key] = result
  ctx.step(`${book.key}: races ${JSON.stringify({ opened: opened.blobUrls, afterPresses: afterPresses.blobUrls, afterHeld: afterHeld.blobUrls, closed: closed.blobUrls, heldSettledMs, doublePressMs: result.doublePressMs })}`)
  check(afterPresses.blobUrls <= MAX_SHOWN_BLOBS, `${book.key}: after ${DOUBLE_PRESSES} double presses only the shown page holds blob URLs`, { opened: opened.blobTypes, afterPresses: afterPresses.blobTypes })
  check(afterHeld.blobUrls <= MAX_SHOWN_BLOBS, `${book.key}: after a held key only the shown page holds blob URLs`, afterHeld.blobTypes)
  check(closed.blobUrls === before.blobUrls && closed.workers === before.workers, `${book.key}: closing after the races gives everything back`, { before, closed })
}

async function traverse(ctx, profile, book, report, check) {
  const before = await ctx.app.evaluate(LIVE)
  await open(ctx, book, "")
  const samples = (report.books[book.key] = [await sample(ctx, profile, 0)])
  let page = 0
  let forward = true
  for (let turn = 1; turn <= (book.turns ?? PAGES); turn += 1) {
    if (page === book.pages - 1) forward = false
    if (page === 0) forward = true
    const before = await ctx.app.evaluate(`document.querySelector("[data-reader-footer]").innerText`)
    await ctx.key(forward ? "ArrowRight" : "ArrowLeft", { keyCode: forward ? 39 : 37 })
    page += forward ? 1 : -1
    await ctx.until(async () => (await ctx.app.evaluate(`document.querySelector("[data-reader-footer]").innerText`)) !== before, `${book.key}: turn ${turn} to page ${page + 1}`, 20000, { quiet: true })
    if (turn % SAMPLE_EVERY === 0) samples.push({ ...(await sample(ctx, profile, turn)), page: page + 1 })
  }
  // A critical memory-pressure signal makes Chromium drop its caches; what
  // stays after it is held by the page, not cached.
  await ctx.app.send("Memory.simulatePressureNotification", { level: "critical" })
  const settle = Date.now() + 3000
  await ctx.until(async () => Date.now() > settle, `${book.key}: 3 s after memory pressure`, 5000, { quiet: true })
  report.afterPressure = { ...report.afterPressure, [book.key]: await sample(ctx, profile, "after pressure") }
  check(report.afterPressure[book.key].pageResults <= MAX_SHOWN_PAGE_RESULTS, `${book.key}: after ${book.turns} turns only the shown page's load result is reachable`, samples.map((entry) => entry.pageResults))
  const functionGrowth = report.afterPressure[book.key].nativeUnnamedFunctions - samples[1].nativeUnnamedFunctions
  check(functionGrowth <= MAX_FUNCTION_GROWTH, `${book.key}: turns ${SAMPLE_EVERY} to ${book.turns} leave no promise functions behind`, { growth: functionGrowth, samples: samples.map((entry) => entry.nativeUnnamedFunctions) })
  ctx.step(`${book.key}: ${samples.map((entry) => `${entry.turns}:${Math.round(entry.rssKiB / 1024)}MiB/${entry.blobUrls}blobs`).join(" ")}, after pressure ${Math.round(report.afterPressure[book.key].rssKiB / 1024)}MiB`)
  const closes = [await close(ctx, book, "")]
  for (let round = 1; round <= REOPENS; round += 1) {
    await open(ctx, book, ` again (${round})`)
    closes.push(await close(ctx, book, ` again (${round})`))
  }
  report.afterClose = { ...report.afterClose, [book.key]: { before, closes } }
  ctx.step(`${book.key}: live before opening ${JSON.stringify(before)}, after each close ${JSON.stringify(closes)}`)
}
