const FIXTURES = {
  trap: { name: "MediaFlick Script Trap", format: "epub" },
  epub: { name: "Alice's Adventures in Wonderland", format: "epub" },
  pdf: { name: "The Constitution of the United States - US Congress", format: "pdf" },
  cbr: { name: "Travels of Timmy Toodles - Johnny Gruelle", format: "cbr" },
}

const EMPTY_WASM = "new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])"

async function bookIds(ctx) {
  const answer = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Book`)
  const ids = {}
  for (const [key, { name }] of Object.entries(FIXTURES)) {
    const item = answer.Items.find((candidate) => candidate.Name === name)
    if (!item) throw new Error(`the Books library has no "${name}"`)
    ids[key] = item.Id
  }
  return ids
}

async function frames(ctx) {
  const { root } = await ctx.app.send("DOM.getDocument", { depth: -1, pierce: true })
  const found = []
  const walk = (node) => {
    if (node.nodeName === "IFRAME") {
      const attributes = Object.fromEntries((node.attributes ?? []).reduce((pairs, value, index, all) => (index % 2 ? pairs : [...pairs, [value, all[index + 1]]]), []))
      const text = []
      const collect = (inner) => {
        if (inner.nodeType === 3) text.push(inner.nodeValue)
        for (const child of [...(inner.children ?? []), ...(inner.shadowRoots ?? [])]) collect(child)
      }
      if (node.contentDocument) collect(node.contentDocument)
      found.push({ sandbox: attributes.sandbox ?? null, src: attributes.src ?? null, text: text.join(" ").replace(/\s+/g, " ").trim().slice(0, 400) })
    }
    for (const child of [...(node.children ?? []), ...(node.shadowRoots ?? []), ...(node.contentDocument ? [node.contentDocument] : [])]) walk(child)
  }
  walk(root)
  return found
}

async function shownPageImages(ctx) {
  const { root } = await ctx.app.send("DOM.getDocument", { depth: -1, pierce: true })
  const images = []
  const walk = (node, inBook) => {
    if (inBook && node.nodeName === "IMG") images.push(node.backendNodeId)
    for (const child of [...(node.children ?? []), ...(node.shadowRoots ?? [])]) walk(child, inBook)
    if (node.contentDocument) walk(node.contentDocument, true)
  }
  walk(root, false)
  let shown = 0
  for (const backendNodeId of images) {
    const box = await ctx.app.send("DOM.getBoxModel", { backendNodeId }).catch(() => null)
    if (box && box.model.width > 0 && box.model.height > 0) shown += 1
  }
  return shown
}

async function open(ctx, id, format, timeout = 120000) {
  await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(`/read/${id}?format=${format}`)}); dispatchEvent(new PopStateEvent("popstate"))`)
  const element = `document.querySelector('[data-reader-book="${id}"]')`
  await ctx.until(`["ready", "failed"].includes(${element}?.dataset.readerPhase)`, `${format} to open or fail`, timeout)
  const phase = await ctx.app.evaluate(`${element}.dataset.readerPhase`)
  return { phase, status: await status(ctx) }
}

async function status(ctx) {
  return ctx.app.evaluate(`document.querySelector('main [role="status"]').textContent`)
}

export default async function (ctx) {
  const report = { console: [], issues: [], formats: {} }
  try {
    await drive(ctx, report)
  } finally {
    report.cspViolations = report.console.filter((entry) => /Content Security Policy/i.test(entry.text))
    ctx.writeEvidence("books.json", JSON.stringify(report, null, 2))
  }
}

async function drive(ctx, report) {
  await ctx.signIn({})
  ctx.app.on("Log.entryAdded", ({ entry }) => report.console.push({ source: entry.source, level: entry.level, text: entry.text, url: entry.url }))
  ctx.app.on("Runtime.consoleAPICalled", ({ type, args }) => report.console.push({ source: "console-api", level: type, text: args.map((arg) => arg.value ?? arg.description).join(" ") }))
  ctx.app.on("Audits.issueAdded", ({ issue }) => report.issues.push(issue))
  report.workers = []
  ctx.app.on("Target.attachedToTarget", ({ targetInfo }) => report.workers.push({ type: targetInfo.type, url: targetInfo.url }))
  await ctx.app.send("Target.setAutoAttach", { autoAttach: true, waitForDebuggerOnStart: false, flatten: true })
  await ctx.app.send("Log.enable")
  await ctx.app.send("Runtime.enable")
  await ctx.app.send("Audits.enable")
  await ctx.app.evaluate(`(() => { window.__bookMessages = []; addEventListener("message", (event) => { if (String(event.data).startsWith("book-script-ran")) window.__bookMessages.push(String(event.data)) }) })()`)
  report.origin = await ctx.app.evaluate("({ location: location.origin, url: new URL(location.href).origin, parsed: URL.parse('/reader/x.js', location.href)?.origin })")
  const ids = await bookIds(ctx)
  report.ids = ids

  report.documentWasm = await ctx.app.evaluate(`WebAssembly.instantiate(${EMPTY_WASM}).then(() => "compiled", (error) => String(error))`)
  ctx.check(report.documentWasm !== "compiled", `the page refuses to compile WebAssembly (${report.documentWasm})`)

  const trap = await open(ctx, ids.trap, "epub")
  ctx.check(trap.phase === "ready", `the script trap EPUB opened (${trap.status})`)
  const settleUntil = Date.now() + 3000
  await ctx.until(async () => Date.now() > settleUntil, "the script trap's 3 s observation window", 5000)
  const trapFrames = await frames(ctx)
  const trapLog = ctx.appLog().split("\n").filter((line) => line.includes("book-script-trap"))
  const trapMessages = await ctx.app.evaluate("window.__bookMessages")
  report.trap = { frames: trapFrames, appLog: trapLog, messages: trapMessages }
  ctx.check(trapFrames.some((frame) => frame.text.includes("MediaFlick's sandbox failed")), "the trap's section rendered inside the reader")
  ctx.check(trapFrames.every((frame) => frame.sandbox === "allow-same-origin" || frame.src === null), `every book iframe is sandboxed without scripts (${JSON.stringify(trapFrames.map((frame) => frame.sandbox))})`)
  ctx.check(trapLog.length === 0, `no trap probe reached the app (${trapLog.length} log lines)`)
  ctx.check(!trapMessages.some((message) => message.startsWith("book-script-ran")), `no trap probe posted to the page (${JSON.stringify(trapMessages)})`)
  await ctx.screenshot("01-script-trap")

  const epub = await open(ctx, ids.epub, "epub")
  ctx.check(epub.phase === "ready", `the EPUB opened (${epub.status})`)
  const { root } = await ctx.app.send("DOM.getDocument", { depth: -1, pierce: true })
  const iframes = []
  const collect = (node) => {
    if (node.nodeName === "IFRAME" && node.attributes?.includes("sandbox")) iframes.push(node)
    for (const child of [...(node.children ?? []), ...(node.shadowRoots ?? [])]) collect(child)
  }
  collect(root)
  await ctx.app.send("DOM.focus", { backendNodeId: iframes[0].backendNodeId })
  const focusInFrame = await ctx.app.evaluate("document.activeElement?.tagName")
  ctx.check(focusInFrame === "FOLIATE-VIEW", `focus is inside the book's iframe (${focusInFrame})`)
  const beforeTurn = await status(ctx)
  for (let page = 0; page < 3; page += 1) await ctx.key("ArrowRight")
  await ctx.until(async () => (await status(ctx)) !== beforeTurn, "ArrowRight inside the sandboxed iframe to turn the EPUB's page", 10000)
  report.formats.epub = { opened: epub, focusAfterClick: focusInFrame, before: beforeTurn, after: await status(ctx) }
  await ctx.screenshot("02-epub")

  const pdf = await open(ctx, ids.pdf, "pdf", 30000)
  ctx.check(pdf.phase === "ready", `the PDF opened (${pdf.status})`)
  const workers = report.workers.map((worker) => worker.url)
  report.formats.pdf = { opened: pdf, workers }
  ctx.check(workers.some((url) => url.endsWith("/reader/foliate/vendor/pdfjs/pdf.worker.mjs")), `pdf.js runs its own worker (${JSON.stringify(workers)})`)
  ctx.check(!report.console.some((entry) => /fake worker/i.test(entry.text)), "pdf.js did not fall back to a fake worker")
  await ctx.until(async () => (await shownPageImages(ctx)) > 0, "the PDF page to show as a laid-out image inside the book's iframe", 20000)
  await ctx.screenshot("03-pdf")

  const cbr = await open(ctx, ids.cbr, "cbr")
  ctx.check(cbr.phase === "ready" && /section 1\/28$/.test(cbr.status), `the RAR5 CBR decoded to its 28 pages (${cbr.status})`)
  ctx.check(report.workers.some((worker) => worker.url.endsWith("/reader/libarchive/worker-bundle.js")), "libarchive decoded in its own worker")
  report.formats.cbr = { opened: cbr }
  await ctx.screenshot("04-cbr")

  report.documentWasmAfter = await ctx.app.evaluate(`WebAssembly.instantiate(${EMPTY_WASM}).then(() => "compiled", (error) => String(error))`)
  ctx.check(report.documentWasmAfter !== "compiled", "the page still refuses WebAssembly after the worker decoded")
}
