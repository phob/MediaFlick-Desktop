// Memory while reading fixed pages (`--account`, Linux): turns 250 pages
// of the 85-page PDF (bouncing between its ends, so pages are read again)
// and 540 of the 271-page CBZ (to its end and back), and every 10 turns records the renderer processes' resident memory (from
// /proc), the JS heap, the document and frame counts, and the page's live blob URLs and workers. A curve that keeps rising with pages read,
// rather than levelling off, means pages are kept after they leave the
// screen. Then it opens and closes each book three more times: what the reader
// leaves alive after it closed must not grow with opens. Writes books-residency.json.
import { readdirSync, readFileSync } from "node:fs"

const BOOKS = [
  { key: "pdf", name: "The Constitution of the United States - US Congress", pages: 85, turns: 250 },
  { key: "cbz", name: "Shen of the Sea - Arthur Bowie Chrisman", pages: 271, turns: 540 },
]
const REOPENS = 3
/** Counts the page's live blob URLs and workers: the reader's engines run in the page itself. */
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
  window.__residency = { blobs, workers }
})()`
const LIVE = `({
  blobUrls: window.__residency.blobs.size,
  blobTypes: [...window.__residency.blobs.values()].reduce((counts, type) => ({ ...counts, [type]: (counts[type] ?? 0) + 1 }), {}),
  workers: window.__residency.workers.size,
})`
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
    ...(await ctx.app.evaluate(LIVE)),
  }
}

export default async function (ctx) {
  await ctx.signIn({})
  await ctx.app.send("Performance.enable")
  await ctx.app.evaluate(INSTRUMENT)
  const profile = ctx.configFile("").split("/config/")[0]
  const answer = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Book`)
  const report = { profile: profile.split("/").pop(), books: {} }
  try {
    for (const book of BOOKS) await traverse(ctx, profile, answer, book, report)
  } finally {
    ctx.writeEvidence("books-residency.json", JSON.stringify(report, null, 2))
  }
}

async function traverse(ctx, profile, answer, book, report) {
  const id = answer.Items.find((item) => item.Name === book.name)?.Id
  ctx.check(Boolean(id), `the Books library has "${book.name}"`)
  ctx.restoreItemAfterRun(id)
  const open = async (label) => {
    await ctx.app.evaluate(`history.pushState(null, "", "/read/${id}?from=start"); dispatchEvent(new PopStateEvent("popstate"))`)
    await ctx.until(`document.querySelector('[data-reader-book="${id}"]')?.dataset.readerPhase === "ready"`, `${book.key}: the reader opened${label}`, 120000)
  }
  const close = async (label) => {
    await ctx.key("Escape", { keyCode: 27 })
    await ctx.until(`!document.querySelector('[data-reader-book="${id}"]')`, `${book.key}: the reader closed${label}`, 20000)
    await ctx.app.send("HeapProfiler.collectGarbage")
    return ctx.app.evaluate(LIVE)
  }
  const before = await ctx.app.evaluate(LIVE)
  await open("")
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
  ctx.step(`${book.key}: ${samples.map((entry) => `${entry.turns}:${Math.round(entry.rssKiB / 1024)}MiB/${entry.blobUrls}blobs`).join(" ")}, after pressure ${Math.round(report.afterPressure[book.key].rssKiB / 1024)}MiB`)
  const closes = [await close("")]
  for (let round = 1; round <= REOPENS; round += 1) {
    await open(` again (${round})`)
    closes.push(await close(` again (${round})`))
  }
  report.afterClose = { ...report.afterClose, [book.key]: { before, closes } }
  ctx.step(`${book.key}: live before opening ${JSON.stringify(before)}, after each close ${JSON.stringify(closes)}`)
}
