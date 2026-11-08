import { execFileSync } from "node:child_process"

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
const EXTRA = [
  // Chapter 5 is the fifth spine item, so its CFI starts /6/10!.
  { key: "nested", name: "MediaFlick Nested Contents", format: "epub", tocEntry: "Section 5.1", tocCfi: "epubcfi(/6/10!" },
  { key: "fxl", name: "MediaFlick Fixed Layout", format: "epub", contentsDowns: 1 },
]
const TRAP = "MediaFlick Script Trap"
const TEXT_FORMATS = new Set(["epub", "mobi", "azw", "azw3"])
const EMPTY_WASM = "new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])"

const phase = (id) => `document.querySelector('[data-reader-book="${id}"]')?.dataset.readerPhase`
const layer = `document.querySelector("[data-reader-layer]")?.dataset.readerLayer`
const focusedEntry = `document.activeElement?.closest("[data-reader-row]")?.dataset.readerEntry ?? null`
const remote = {
  menu: ["ContextMenu", { code: "ContextMenu", keyCode: 93 }],
  back: ["BrowserBack", { code: "BrowserBack", keyCode: 166 }],
  up: ["ArrowUp", { code: "ArrowUp", keyCode: 38 }],
  down: ["ArrowDown", { code: "ArrowDown", keyCode: 40 }],
  left: ["ArrowLeft", { code: "ArrowLeft", keyCode: 37 }],
  right: ["ArrowRight", { code: "ArrowRight", keyCode: 39 }],
  pageUp: ["PageUp", { code: "PageUp", keyCode: 33 }],
  pageDown: ["PageDown", { code: "PageDown", keyCode: 34 }],
}
const press = (ctx, name) => ctx.key(...remote[name])
const footer = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll("[data-reader-footer] > span")].map((span) => span.innerText).join(" · ")`)

/** Enter as a keyboard sends it: a key-down that also types `\r`, which is what activates a focused link or button. */
async function enter(ctx) {
  const event = { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 }
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyDown", text: "\r", unmodifiedText: "\r", ...event })
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyUp", ...event })
}

function expectedTicks(location) {
  return location.kind === "text" ? Math.round(location.fraction * 1e7) : location.page * 1e4
}

async function moved(ctx, book, previous, description) {
  const saved = await ctx.until(() => {
    const current = place(ctx, book.id)
    const changed = current && JSON.stringify(current.location) !== JSON.stringify(previous)
    return changed && current.syncedTicks === expectedTicks(current.location) && current
  }, `${book.key}: ${description}`, 15000)
  const shown = /Page (\d+) of (\d+)/.exec(await footer(ctx))
  if (shown && shown[1] === shown[2]) book.reachedEnd = true
  return saved.location
}

/** The saved place once it held still for two seconds: a new text size lays the book out again. */
async function settled(ctx, book) {
  let last = JSON.stringify(place(ctx, book.id))
  let since = Date.now()
  await ctx.until(() => {
    const now = JSON.stringify(place(ctx, book.id))
    if (now !== last) [last, since] = [now, Date.now()]
    return Date.now() - since > 2000
  }, `${book.key}: the saved place to settle`, 20000)
  return place(ctx, book.id)
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
  await panels(ctx, book, row)
  const saved = await settled(ctx, book)
  const server = await ctx.jellyfin(`/Items/${book.id}?userId=${ctx.account.userId}`)
  row.place = saved
  row.jellyfin = { ticks: server.UserData.PlaybackPositionTicks, played: server.UserData.Played, expected: expectedTicks(saved.location) }
  ctx.check(row.jellyfin.ticks === row.jellyfin.expected, `${book.key}: Jellyfin holds ${row.jellyfin.expected} ticks in jellyfin-web's units (${row.jellyfin.ticks})`)
  ctx.check(row.jellyfin.played === Boolean(book.reachedEnd), `${book.key}: Jellyfin marks it played only once its last page was shown (${row.jellyfin.played})`)
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

async function inLayer(ctx, book, name, how) {
  const focused = name === "reading" ? "true" : `Boolean(document.activeElement?.closest("[data-reader-panel='${name}'] [data-reader-row]"))`
  await ctx.until(`${layer} === "${name}" && ${focused}`, `${book.key}: ${how}`)
}

async function chooseFromMenu(ctx, book, entry) {
  for (let step = 0; step < 10 && (await ctx.app.evaluate(focusedEntry)) !== entry; step += 1) await press(ctx, "down")
  ctx.check((await ctx.app.evaluate(focusedEntry)) === entry, `${book.key}: the menu focuses ${entry}`)
  await enter(ctx)
}

async function widthScroll(ctx, book) {
  const page = async () => Number(/Page (\d+) of/.exec(await footer(ctx))?.[1])
  const start = await page()
  let now = start
  let downs = 0
  while (downs < 12 && now === start) {
    await press(ctx, "down")
    downs += 1
    const wait = Date.now() + 400
    await ctx.until(async () => Date.now() > wait, "a scroll step", 2000, { quiet: true })
    now = await page()
  }
  ctx.check(downs > 1 && now === start + 1, `${book.key}: Down scrolls the wide page ${downs - 1} times before page ${start + 1} (now ${now})`)
  return { start, downs, end: now }
}

async function panels(ctx, book, row) {
  const steps = (row.panels = {})
  await press(ctx, "menu")
  await inLayer(ctx, book, "menu", "the Menu key opens the menu")
  await ctx.screenshot(`${book.key}-03-menu`)
  await chooseFromMenu(ctx, book, "toc")
  await inLayer(ctx, book, "toc", "Contents opens from the menu")
  const start = await ctx.app.evaluate(focusedEntry)
  if (book.tocEntry) {
    for (let step = 0; step < 40 && (await ctx.app.evaluate("document.activeElement?.innerText?.trim()")) !== book.tocEntry; step += 1) await press(ctx, "down")
  } else {
    for (let step = 0; step < (book.contentsDowns ?? 3); step += 1) await press(ctx, "down")
  }
  const chosen = { entry: await ctx.app.evaluate(focusedEntry), label: await ctx.app.evaluate("document.activeElement?.innerText?.trim()") }
  await ctx.screenshot(`${book.key}-04-contents`)
  const before = place(ctx, book.id)?.location ?? null
  await enter(ctx)
  await inLayer(ctx, book, "reading", "choosing an entry returns to the book")
  const jumped = await moved(ctx, book, before, `the jump to "${chosen.label}"`)
  steps.contents = { start, chosen, location: jumped, footer: await footer(ctx) }
  // A two-column spread can show the next entry too, and foliate names the page after it.
  if (book.tocEntry) ctx.check(jumped.cfi.startsWith(book.tocCfi) && (await footer(ctx)).startsWith("Section 5."), `${book.key}: the nested entry ${book.tocEntry} opens (${jumped.cfi}, ${await footer(ctx)})`)
  if (jumped.kind === "page") ctx.check(jumped.page === Number(chosen.entry), `${book.key}: Contents opens page ${Number(chosen.entry) + 1} (${jumped.page + 1})`)

  await press(ctx, "menu")
  await inLayer(ctx, book, "menu", "the menu opens again")
  await chooseFromMenu(ctx, book, "goto")
  await inLayer(ctx, book, "goto", "Go to opens from the menu")
  const slider = await ctx.app.evaluate(`({ min: Number(document.activeElement.getAttribute("aria-valuemin")), max: Number(document.activeElement.getAttribute("aria-valuemax")), now: Number(document.activeElement.getAttribute("aria-valuenow")), role: document.activeElement.getAttribute("role") })`)
  ctx.check(slider.role === "slider", `${book.key}: Go to focuses its slider`)
  const fixed = slider.min === 1
  const step = fixed ? 1 : 0.5
  const target = fixed ? Math.round(slider.max / 2) : 50
  const now = () => ctx.app.evaluate(`Number(document.activeElement.getAttribute("aria-valuenow"))`)
  // A slider follows ARIA: Page Up raises the value by ten steps, Page Down lowers it.
  for (let presses = 0; presses < 60; presses += 1) {
    const distance = target - (await now())
    if (Math.abs(distance) < step / 2) break
    const far = Math.abs(distance) >= step * 10
    await press(ctx, distance > 0 ? (far ? "pageUp" : "right") : far ? "pageDown" : "left")
  }
  const value = await now()
  await ctx.screenshot(`${book.key}-05-goto`)
  const beforeGo = place(ctx, book.id).location
  await enter(ctx)
  await inLayer(ctx, book, "reading", "Enter goes there and returns to the book")
  const went = await moved(ctx, book, beforeGo, `the jump to ${value}${fixed ? "" : "%"}`)
  steps.goto = { slider, value, location: went, footer: await footer(ctx) }
  ctx.check(Math.abs(value - target) < step, `${book.key}: the slider reaches ${target} with Page Up/Down and arrows (${value})`)
  // A text book opens at the spread holding that fraction; in a short book one spread is several percent.
  const opened = fixed ? (await footer(ctx)).includes(`Page ${value} of `) : Math.abs(went.fraction - 0.5) < 0.1
  ctx.check(opened, `${book.key}: Go to opens about halfway (${JSON.stringify(went)}, ${await footer(ctx)})`)

  await press(ctx, "menu")
  await inLayer(ctx, book, "menu", "the menu opens a third time")
  await chooseFromMenu(ctx, book, "settings")
  await inLayer(ctx, book, "settings", "Appearance opens from the menu")
  const appearance = ctx.readConfig("reader.json")?.accounts?.[0]?.appearance ?? { fontSize: 100, theme: "app", fit: "page" }
  const firstRow = await ctx.app.evaluate(`document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.closest("[role=radiogroup]")?.getAttribute("aria-label")`)
  if (firstRow === "Text size") {
    await press(ctx, appearance.fontSize < 200 ? "right" : "left")
    const size = appearance.fontSize < 200 ? appearance.fontSize + 10 : appearance.fontSize - 10
    await ctx.until(() => ctx.readConfig("reader.json")?.accounts?.[0]?.appearance?.fontSize === size, `${book.key}: the text size changes to ${size}% and is kept`)
  } else {
    await press(ctx, "right")
    await ctx.until(() => ctx.readConfig("reader.json")?.accounts?.[0]?.appearance?.fit === (appearance.fit === "page" ? "width" : "page"), `${book.key}: the fit changes and is kept`)
  }
  await press(ctx, "down")
  const themeRow = await ctx.app.evaluate(`document.activeElement?.closest("[role=radiogroup]")?.getAttribute("aria-label")`)
  ctx.check(themeRow === "Theme", `${book.key}: Down moves to the theme row (${themeRow})`)
  await press(ctx, "right")
  await ctx.until(() => ctx.readConfig("reader.json")?.accounts?.[0]?.appearance?.theme !== appearance.theme, `${book.key}: Right changes the theme and it is kept`)
  steps.appearance = { firstRow, before: appearance, after: ctx.readConfig("reader.json").accounts[0].appearance }
  await ctx.screenshot(`${book.key}-06-appearance`)
  await press(ctx, "back")
  await inLayer(ctx, book, "menu", "Back closes Appearance into the menu")
  ctx.check((await ctx.app.evaluate(focusedEntry)) === "settings", `${book.key}: the menu focuses Appearance again`)
  await press(ctx, "back")
  await inLayer(ctx, book, "reading", "Back closes the menu")
  if (steps.appearance.after.fit === "width" && firstRow === "Fit") steps.widthScroll = await widthScroll(ctx, book)
  await ctx.screenshot(`${book.key}-07-reading-after`)
}

async function keyboardPass(ctx, book, report) {
  const row = (report.keyboard = { book: book.key, steps: [] })
  await openDetail(ctx, book)
  await readByKeyboard(ctx, book, row)
  for (const [key, panel, close] of [["t", "toc", "Escape"], ["g", "goto", "Backspace"], ["a", "settings", "Escape"], ["?", "keys", "Backspace"]]) {
    await ctx.key(key)
    await inLayer(ctx, book, panel, `${key} opens ${panel}`)
    if (panel === "keys") await ctx.screenshot("keyboard-keys-sheet")
    await ctx.key(close, { keyCode: close === "Escape" ? 27 : 8 })
    await inLayer(ctx, book, "reading", `${close} closes ${panel}`)
    row.steps.push({ key, panel, close })
  }
  const size = ctx.readConfig("reader.json").accounts[0].appearance.fontSize
  const bigger = Math.min(200, size + 10)
  await ctx.key("+")
  await ctx.until(() => ctx.readConfig("reader.json").accounts[0].appearance.fontSize === bigger, `+ makes the text ${bigger}%`)
  await ctx.key("-")
  await ctx.until(() => ctx.readConfig("reader.json").accounts[0].appearance.fontSize === bigger - 10, "- makes the text smaller again")
  const before = (await settled(ctx, book)).location
  await ctx.key("t")
  await inLayer(ctx, book, "toc", "t opens Contents for a jump")
  for (let step = 0; step < 4; step += 1) await ctx.key("ArrowDown", { keyCode: 40 })
  await enter(ctx)
  const jumped = await moved(ctx, book, before, "a jump from Contents")
  await ctx.key("b")
  const returned = await moved(ctx, book, jumped, "b to return to where the jump started")
  row.returned = { before, jumped, returned }
  ctx.check(Math.abs(returned.fraction - before.fraction) < 0.02, `b returns near where the jump started (${before.fraction} to ${returned.fraction})`)
  await ctx.key("Escape", { keyCode: 27 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${book.id}`, "Escape leaves the reader")
}

function appWindow() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) .*?(\d+)x(\d+)\+/gm)]
    .map(([, id, width, height]) => ({ id, area: Number(width) * Number(height) }))
    .sort((a, b) => b.area - a.area)[0].id
}

/**
 * Real X11 key presses (XTEST), as a TV remote's keys arrive, after a real
 * click into the book: Return, Right, Next/Previous track, Menu, Down,
 * Page Up and Back. Linux only.
 */
async function remoteX11Pass(ctx, book, report) {
  const row = (report.x11 = { book: book.key, steps: [] })
  const window = appWindow()
  const x = (...args) => execFileSync("xdotool", args)
  await openDetail(ctx, book)
  x("mousemove", "--window", window, "1270", "10")
  x("windowfocus", "--sync", window)
  await ctx.until(`document.activeElement?.getAttribute("href")?.startsWith("/read/")`, "x11: the read action has the focus")
  x("key", "Return")
  await ctx.until(`${phase(book.id)} === "ready"`, "x11: Return opens the book", 120000)
  const opened = await moved(ctx, book, place(ctx, book.id)?.location ?? null, "x11: the opened place saved")
  x("mousemove", "--window", window, "640", "380", "click", "1")
  const focus = await ctx.until(`document.activeElement?.tagName === "FOLIATE-VIEW" && document.activeElement.tagName`, "x11: a click puts the focus inside the book's iframe")
  row.steps.push({ focus })
  x("key", "Right")
  const turned = await moved(ctx, book, opened, "x11: Right inside the iframe turns the page")
  x("key", "XF86AudioNext")
  const chapter = await moved(ctx, book, turned, "x11: Next track inside the iframe moves to the next chapter")
  x("key", "XF86AudioPrev")
  const back = await moved(ctx, book, chapter, "x11: Previous track moves back a chapter")
  row.steps.push({ opened, turned, chapter, back })
  x("key", "Menu")
  await inLayer(ctx, book, "menu", "x11: Menu opens the menu")
  x("key", "Down")
  await ctx.until(`${focusedEntry} === "goto"`, "x11: Down moves to Go to")
  x("key", "Return")
  await inLayer(ctx, book, "goto", "x11: Return opens Go to")
  const valueBefore = await ctx.app.evaluate(`Number(document.activeElement.getAttribute("aria-valuenow"))`)
  x("key", "Prior")
  await ctx.until(async () => Number(await ctx.app.evaluate(`document.activeElement.getAttribute("aria-valuenow")`)) > valueBefore, "x11: Page Up raises the position")
  x("key", "Return")
  await inLayer(ctx, book, "reading", "x11: Return goes there")
  const went = await moved(ctx, book, back, "x11: the jump saved")
  row.steps.push({ valueBefore, went })
  x("key", "Menu")
  await inLayer(ctx, book, "menu", "x11: Menu opens the menu again")
  x("key", "XF86Back")
  await inLayer(ctx, book, "reading", "x11: Back closes the menu")
  await ctx.screenshot("x11-reading")
  x("key", "XF86Back")
  await ctx.until(async () => (await ctx.pathname()) === `/item/${book.id}`, "x11: Back leaves the reader")
}

async function api(ctx, report) {
  const formats = {}
  for (const book of [...BOOKS, ...EXTRA]) {
    const manifest = await ctx.api(`/api/item/${book.id}/book`)
    formats[book.key] = manifest.body?.format ?? `HTTP ${manifest.status}`
    ctx.check(manifest.status === 200 && manifest.body.format === book.format, `api: ${book.key} sniffs as ${book.format} (${formats[book.key]})`)
  }
  const big = BOOKS.find((book) => book.measureBytes).id
  const bytes = await ctx.app.evaluate(`(async () => {
    const ranged = await fetch("/api/item/${big}/book/file?start=0&end=1023")
    const whole = await fetch("/api/item/${big}/book/file")
    return { ranged: [ranged.status, ranged.headers.get("Content-Range"), (await ranged.arrayBuffer()).byteLength],
      unbounded: [whole.status, whole.headers.get("Content-Range"), (await whole.arrayBuffer()).byteLength] }
  })()`)
  ctx.check(bytes.ranged[0] === 206 && bytes.ranged[2] === 1024, `api: a ranged read answers 206 with 1024 bytes (${JSON.stringify(bytes.ranged)})`)
  ctx.check(bytes.unbounded[0] === 206 && bytes.unbounded[2] === 8 * 1024 * 1024, `api: a read without a range answers one 8 MiB chunk (${JSON.stringify(bytes.unbounded)})`)
  const audiobook = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=AudioBook&Limit=1`)).Items[0]
  const movie = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Movie&Limit=1`)).Items[0]
  ctx.check(Boolean(audiobook && movie), "api: the server has an audiobook and a movie")
  ctx.restoreItemAfterRun(audiobook.Id)
  const audiobookLayout = (await ctx.api(`/api/item/${audiobook.Id}`)).body.layout
  ctx.check(audiobookLayout === "audio", `api: an audiobook answers the audio page (${audiobookLayout})`)
  const audiobookManifest = (await ctx.api(`/api/item/${audiobook.Id}/book`)).status
  ctx.check(audiobookManifest === 404, `api: the reader refuses an audiobook (HTTP ${audiobookManifest})`)
  const unopened = await ctx.app.evaluate(`(async () => {
    const place = { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ location: { kind: "page", page: 0, pages: 1 }, finished: false }) }
    return {
      movieFile: (await fetch("/api/item/${movie.Id}/book/file?start=0&end=1023")).status,
      audiobookFile: (await fetch("/api/item/${audiobook.Id}/book/file?start=0&end=1023")).status,
      audiobookPlace: (await fetch("/api/item/${audiobook.Id}/book/position", place)).status,
    }
  })()`)
  ctx.check(Object.values(unopened).every((status) => status === 404), `api: no bytes and no place for an item the reader has not opened (${JSON.stringify(unopened)})`)
  report.api = { formats, bytes, audiobookLayout, audiobookManifest, unopened }
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
    for (const book of [...BOOKS, ...EXTRA]) {
      book.id = report.ids[book.name]
      ctx.check(Boolean(book.id), `the Books library has "${book.name}"`)
    }
    await api(ctx, report)
    report.documentWasm = await ctx.app.evaluate(`WebAssembly.instantiate(${EMPTY_WASM}).then(() => "compiled", (error) => String(error))`)
    ctx.check(report.documentWasm !== "compiled", "the page refuses to compile WebAssembly")
    await scriptTrap(ctx, report)
    const only = process.env.MEDIAFLICK_VERIFY_BOOKS?.split(",")
    for (const book of [...BOOKS, ...EXTRA].filter((candidate) => !only || only.includes(candidate.key))) await readFormat(ctx, book, report)
    const sherlock = BOOKS.find((book) => book.key === "epub2")
    if (!only || only.includes("keyboard")) await keyboardPass(ctx, sherlock, report)
    if (process.platform === "linux" && (!only || only.includes("x11"))) await remoteX11Pass(ctx, BOOKS.find((book) => book.key === "azw"), report)
    report.documentWasmAfter = await ctx.app.evaluate(`WebAssembly.instantiate(${EMPTY_WASM}).then(() => "compiled", (error) => String(error))`)
    ctx.check(report.documentWasmAfter !== "compiled", "the page still refuses WebAssembly after the archive worker ran")
  } finally {
    report.cspViolations = report.issues.filter((issue) => issue.code === "ContentSecurityPolicyIssue" && issue.details.contentSecurityPolicyIssueDetails.sourceCodeLocation?.url !== "")
    report.page = await ctx.app.evaluate("document.documentElement.outerHTML").catch(() => null)
    ctx.writeEvidence("books.json", JSON.stringify(report, null, 2))
  }
  ctx.check(report.cspViolations.length === 0, `no Content Security Policy violation outside the drive's own probes (${report.cspViolations.length})`)
}
