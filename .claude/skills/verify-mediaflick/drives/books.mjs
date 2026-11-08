// The book reader on a real server (`--account`), per format: the Books
// library, the book's page, Read by keyboard, page turns, chapter keys, the
// position Jellyfin keeps, Back, and Continue at the exact place. Writes
// books.json and screenshots per format. See features/books.md.
const BOOKS = [
  { key: "epub", name: "Alice's Adventures in Wonderland", format: "epub", finish: true },
  { key: "epub3", name: "Pride and Prejudice", format: "epub", measureBytes: true },
  { key: "epub2", name: "The Adventures of Sherlock Holmes", format: "epub" },
  { key: "mobi", name: "Dracula - Bram Stoker", format: "mobi" },
  { key: "azw", name: "Treasure Island - Robert Louis Stevenson", format: "azw" },
  { key: "azw3", name: "Frankenstein - Mary Shelley", format: "azw3" },
  { key: "pdf", name: "The Constitution of the United States - US Congress", format: "pdf", worker: "/reader/foliate/vendor/pdfjs/pdf.worker.mjs" },
  { key: "cbz", name: "Shen of the Sea - Arthur Bowie Chrisman", format: "cbz", pages: 271 },
  { key: "cbt", name: "Shen of the Sea Part One - Arthur Bowie Chrisman", format: "cbt", pages: 30, finish: true },
  { key: "cb7", name: "Shen of the Sea Part Two - Arthur Bowie Chrisman", format: "cb7", pages: 30 },
  { key: "cbr", name: "Travels of Timmy Toodles - Johnny Gruelle", format: "cbr", pages: 28, worker: "/reader/libarchive/worker-bundle.js" },
]
const TRAP = "MediaFlick Script Trap"
const TEXT_FORMATS = new Set(["epub", "mobi", "azw", "azw3"])
const EMPTY_WASM = "new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])"

const phase = (id) => `document.querySelector('[data-reader-book="${id}"]')?.dataset.readerPhase`
const footer = (ctx) => ctx.app.evaluate(`document.querySelector("[data-reader-footer]")?.innerText ?? ""`)

/** Enter as a keyboard sends it: a key-down that also types `\r`, which is what activates a focused link or button. */
async function enter(ctx) {
  const event = { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 }
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyDown", text: "\r", unmodifiedText: "\r", ...event })
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyUp", ...event })
}

function expectedTicks(location) {
  return location.kind === "text" ? Math.round(location.fraction * 1e7) : location.page * 1e4
}

/** Waits until the reader saved a place other than `previous`, synced to Jellyfin. */
async function moved(ctx, book, previous, description) {
  const saved = await ctx.until(() => {
    const current = place(ctx, book.id)
    const changed = current && JSON.stringify(current.location) !== JSON.stringify(previous)
    return changed && current.syncedTicks === expectedTicks(current.location) && current
  }, `${book.key}: ${description}`, 15000)
  return saved.location
}

function place(ctx, id) {
  return ctx.readConfig("reader.json")?.accounts?.[0]?.books?.find((book) => book.itemId === id) ?? null
}

async function bookFrames(ctx) {
  const { root } = await ctx.app.send("DOM.getDocument", { depth: -1, pierce: true })
  const frames = []
  const walk = (node) => {
    if (node.nodeName === "IFRAME" && node.attributes?.includes("sandbox")) {
      const attributes = Object.fromEntries(node.attributes.flatMap((value, index, all) => (index % 2 ? [] : [[value, all[index + 1]]])))
      const text = []
      const collect = (inner) => {
        if (inner.nodeType === 3) text.push(inner.nodeValue)
        for (const child of [...(inner.children ?? []), ...(inner.shadowRoots ?? [])]) collect(child)
      }
      if (node.contentDocument) collect(node.contentDocument)
      frames.push({ backendNodeId: node.backendNodeId, sandbox: attributes.sandbox, text: text.join(" ").replace(/\s+/g, " ").trim() })
    }
    for (const child of [...(node.children ?? []), ...(node.shadowRoots ?? []), ...(node.contentDocument ? [node.contentDocument] : [])]) walk(child)
  }
  walk(root)
  return frames
}

async function openDetail(ctx, book) {
  await ctx.press({ role: "link", name: "Books" })
  await ctx.press({ role: "link", name: `Open details for ${book.name}` })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${book.id}`, `${book.key}: the book's page`)
}

async function readByKeyboard(ctx, book, row) {
  row.action = await ctx.until(
    `document.activeElement?.getAttribute("href")?.startsWith("/read/") && document.activeElement.innerText.trim()`,
    `${book.key}: the page focuses its read action`,
  )
  await enter(ctx)
  await ctx.until(`["ready", "failed"].includes(${phase(book.id)})`, `${book.key}: the reader to open`, 180000)
  const opened = await ctx.app.evaluate(phase(book.id))
  ctx.check(opened === "ready", `${book.key}: the reader opened (${opened} ${await ctx.app.evaluate(`document.querySelector('[role="alert"]')?.innerText ?? ""`)})`)
}

async function readFormat(ctx, book, report) {
  const row = (report.formats[book.key] = { name: book.name, format: book.format })
  ctx.restoreItemAfterRun(book.id)
  await openDetail(ctx, book)
  if (book.measureBytes) await ctx.app.evaluate(`window.__bookBytes = { total: 0, requests: 0, beforeFirstPage: null }`)
  await readByKeyboard(ctx, book, row)
  if (book.measureBytes) {
    row.bytes = await ctx.app.evaluate("window.__bookBytes")
    const range = await ctx.app.evaluate(`fetch("/api/item/${book.id}/book/file?start=0&end=0").then((r) => r.headers.get("Content-Range"))`)
    row.bytes.fileSize = Number(range.split("/")[1])
    ctx.check(row.bytes.beforeFirstPage < row.bytes.fileSize / 2, `${book.key}: ${row.bytes.beforeFirstPage} of ${row.bytes.fileSize} bytes fetched before the first page`)
  }
  await ctx.screenshot(`${book.key}-01-reading`)
  const opened = await moved(ctx, book, null, "the opened place saved")
  row.opened = { footer: await footer(ctx), location: opened }
  for (let turn = 0; turn < 3; turn += 1) await ctx.key("ArrowRight")
  const turned = await moved(ctx, book, opened, "ArrowRight to turn pages")
  row.turned = { footer: await footer(ctx), location: turned }
  if (turned.kind === "page") ctx.check(turned.page === opened.page + 3, `${book.key}: three ArrowRights turn three pages (${opened.page} to ${turned.page})`)
  await ctx.key("MediaTrackNext", { code: "MediaTrackNext", keyCode: 176 })
  const chapter = await moved(ctx, book, turned, "the next-chapter key to move on")
  row.chapter = { footer: await footer(ctx), location: chapter }
  ctx.check(chapter.kind === "page" ? chapter.page > turned.page : chapter.fraction > turned.fraction, `${book.key}: the next-chapter key moves forward`)
  const saved = place(ctx, book.id)
  const server = await ctx.jellyfin(`/Items/${book.id}?userId=${ctx.account.userId}`)
  row.place = saved
  row.jellyfin = { ticks: server.UserData.PlaybackPositionTicks, played: server.UserData.Played, expected: expectedTicks(saved.location) }
  ctx.check(row.jellyfin.ticks === row.jellyfin.expected && !row.jellyfin.played, `${book.key}: Jellyfin holds ${row.jellyfin.expected} ticks in jellyfin-web's units (${row.jellyfin.ticks})`)
  ctx.check(TEXT_FORMATS.has(book.format) === (saved.location.kind === "text"), `${book.key}: the place is kept as a ${saved.location.kind} location`)
  if (book.pages) ctx.check(saved.location.pages === book.pages, `${book.key}: all ${book.pages} pages (${saved.location.pages})`)
  if (book.worker) ctx.check(report.workers.some((url) => url.endsWith(book.worker)), `${book.key}: decoded in its own worker`)
  await ctx.key("BrowserBack", { code: "BrowserBack", keyCode: 166 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${book.id}`, `${book.key}: Back returns to the book's page`)
  row.continueLabel = await ctx.until(`document.activeElement?.innerText?.trim().startsWith("Continue") && document.activeElement.innerText.trim()`, `${book.key}: the page offers Continue`)
  await ctx.screenshot(`${book.key}-02-continue`)
  const before = ctx.configStat("reader.json").mtimeMs
  await readByKeyboard(ctx, book, row)
  await ctx.until(() => ctx.configStat("reader.json").mtimeMs > before, `${book.key}: the reopened place reported`, 15000)
  row.reopened = { footer: await footer(ctx), place: place(ctx, book.id) }
  ctx.check(JSON.stringify(row.reopened.place.location) === JSON.stringify(saved.location), `${book.key}: Continue restores the exact place (${JSON.stringify(row.reopened.place.location)})`)
  row.resources = await ctx.app.evaluate(`performance.getEntriesByType("resource").map((entry) => entry.name).filter((name) => name.includes("/book"))`)
  if (book.finish) await finish(ctx, book, row)
  await ctx.key("Escape")
  await ctx.until(async () => (await ctx.pathname()) === `/item/${book.id}`, `${book.key}: Escape leaves the reader`)
}

/** End shows the last page, which marks the book played; the page then offers Read again, which opens at the start. */
async function finish(ctx, book, row) {
  await ctx.key("End")
  await ctx.until(async () => (await ctx.jellyfin(`/Items/${book.id}?userId=${ctx.account.userId}`)).UserData.Played, `${book.key}: the last page marks the book played in Jellyfin`, 15000)
  await ctx.screenshot(`${book.key}-03-end`)
  await ctx.key("BrowserBack", { code: "BrowserBack", keyCode: 166 })
  row.finished = await ctx.until(`document.activeElement?.innerText?.trim() === "Read again" && document.body.innerText.includes("Finished")`, `${book.key}: the page offers Read again`)
  await readByKeyboard(ctx, book, row)
  const start = await ctx.until(() => {
    const current = place(ctx, book.id)
    return current && (current.location.kind === "page" ? current.location.page === 0 : current.location.fraction < 0.01) && current.location
  }, `${book.key}: Read again opens at the start`, 15000)
  row.readAgain = start
}

async function scriptTrap(ctx, report) {
  const book = { key: "trap", name: TRAP, id: report.ids[TRAP] }
  ctx.restoreItemAfterRun(book.id)
  await openDetail(ctx, book)
  await readByKeyboard(ctx, book, {})
  const until = Date.now() + 3000
  await ctx.until(async () => Date.now() > until, "the script trap's 3 s observation window", 5000)
  const frames = await bookFrames(ctx)
  const probes = ctx.appLog().split("\n").filter((line) => line.includes("book-script-trap"))
  const messages = await ctx.app.evaluate("window.__bookMessages")
  report.trap = { frames: frames.map(({ sandbox, text }) => ({ sandbox, text: text.slice(0, 200) })), probes, messages }
  ctx.check(frames.some((frame) => frame.text.includes("MediaFlick's sandbox failed")), "the script trap's section rendered")
  ctx.check(frames.every((frame) => frame.sandbox === "allow-same-origin"), "every book iframe is sandboxed without scripts")
  ctx.check(probes.length === 0 && messages.length === 0, `no script in the book ran (${probes.length} requests, ${messages.length} messages)`)
  await ctx.screenshot("trap-reading")
  await ctx.key("Escape")
  await ctx.until(async () => (await ctx.pathname()) === `/item/${book.id}`, "Escape leaves the script trap")
}

async function instrument(ctx, report) {
  ctx.app.on("Log.entryAdded", ({ entry }) => report.console.push({ source: entry.source, level: entry.level, text: entry.text, url: entry.url }))
  ctx.app.on("Audits.issueAdded", ({ issue }) => report.issues.push(issue))
  ctx.app.on("Target.attachedToTarget", ({ targetInfo }) => report.workers.push(targetInfo.url))
  await ctx.app.send("Log.enable")
  await ctx.app.send("Audits.enable")
  await ctx.app.send("Target.setAutoAttach", { autoAttach: true, waitForDebuggerOnStart: false, flatten: true })
  await ctx.app.evaluate(`(() => {
    window.__bookMessages = []
    addEventListener("message", (event) => { if (String(event.data).startsWith("book-script-ran")) window.__bookMessages.push(String(event.data)) })
    const original = window.fetch
    window.fetch = async (...args) => {
      const response = await original(...args)
      const url = String(args[0]?.url ?? args[0])
      const range = /bytes (\\d+)-(\\d+)/.exec(response.headers.get("Content-Range") ?? "")
      if (window.__bookBytes && url.includes("/book/file") && range) {
        window.__bookBytes.total += Number(range[2]) - Number(range[1]) + 1
        window.__bookBytes.requests += 1
      }
      return response
    }
    new MutationObserver(() => {
      if (window.__bookBytes?.beforeFirstPage === null && document.querySelector('[data-reader-phase="ready"]')) window.__bookBytes.beforeFirstPage = window.__bookBytes.total
    }).observe(document.body, { subtree: true, attributes: true, attributeFilter: ["data-reader-phase"] })
  })()`)
}

export default async function (ctx) {
  const report = { console: [], issues: [], workers: [], formats: {} }
  try {
    await ctx.signIn({})
    await instrument(ctx, report)
    const answer = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Book`)
    report.ids = Object.fromEntries(answer.Items.map((item) => [item.Name, item.Id]))
    for (const book of BOOKS) {
      book.id = report.ids[book.name]
      ctx.check(Boolean(book.id), `the Books library has "${book.name}"`)
    }
    report.documentWasm = await ctx.app.evaluate(`WebAssembly.instantiate(${EMPTY_WASM}).then(() => "compiled", (error) => String(error))`)
    ctx.check(report.documentWasm !== "compiled", "the page refuses to compile WebAssembly")
    await scriptTrap(ctx, report)
    const only = process.env.MEDIAFLICK_VERIFY_BOOKS?.split(",")
    for (const book of BOOKS.filter((candidate) => !only || only.includes(candidate.key))) await readFormat(ctx, book, report)
    report.documentWasmAfter = await ctx.app.evaluate(`WebAssembly.instantiate(${EMPTY_WASM}).then(() => "compiled", (error) => String(error))`)
    ctx.check(report.documentWasmAfter !== "compiled", "the page still refuses WebAssembly after the archive worker ran")
  } finally {
    report.cspViolations = report.issues.filter((issue) => issue.code === "ContentSecurityPolicyIssue" && issue.details.contentSecurityPolicyIssueDetails.sourceCodeLocation?.url !== "")
    report.page = await ctx.app.evaluate("document.documentElement.outerHTML").catch(() => null)
    ctx.writeEvidence("books.json", JSON.stringify(report, null, 2))
  }
  ctx.check(report.cspViolations.length === 0, `no Content Security Policy violation outside the drive's own probes (${report.cspViolations.length})`)
}
