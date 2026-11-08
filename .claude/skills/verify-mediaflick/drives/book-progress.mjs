// A book's place while Jellyfin's progress writes fail or are slow. The app
// signs in through a loopback proxy that refuses or holds the book's
// `POST /UserItems/<id>/UserData`. Writes the test user's progress and puts it back.
//   just verify book-progress --account SERVER --signed-out --timeout 600
const BOOK = "Shen of the Sea - Arthur Bowie Chrisman"
const PAGES = 271
const SERVER_PAGE = 3

const footer = `[...document.querySelectorAll("[data-reader-footer] > span")].map((span) => span.innerText).join(" · ")`

function deferred() {
  let release
  const promise = new Promise((resolve) => (release = resolve))
  return { promise, release }
}

async function enter(ctx) {
  const event = { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 }
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyDown", text: "\r", unmodifiedText: "\r", ...event })
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyUp", ...event })
}

export default async function bookProgress(ctx) {
  const report = { checks: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` ${JSON.stringify(detail)}`}`)
  }
  const book = (await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Book&SearchTerm=${encodeURIComponent("Shen of the Sea")}`))
    .Items.find((item) => item.Name === BOOK)
  if (!book) throw new Error(`the server has no book "${BOOK}"`)
  const writePath = `/UserItems/${book.Id}/UserData`
  const isWrite = (url, method) => method === "POST" && url.pathname.endsWith(writePath)
  let refuse = false
  let holdNext = null
  const proxy = await ctx.proxy({
    upstream: ctx.account.server,
    answer: (url, method) => (refuse && isWrite(url, method) ? { status: 503, body: { message: "refused by the drive" } } : null),
    delay: (url, method) => {
      if (!isWrite(url, method) || !holdNext) return 0
      const gate = holdNext
      holdNext = null
      return gate.promise
    },
  })
  await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
  await ctx.quickConnect({ approve: true, timeout: 60000 })
  ctx.revokeAppSessionAfterRun()
  ctx.restoreItemAfterRun(book.Id)
  await ctx.until(async () => (await ctx.api("/api/status")).body?.bootstrap?.state === "ready", "the first catalog page through the proxy", 300000)
  await ctx.setResumeOnServer(book.Id, SERVER_PAGE * 1e4)

  const place = () => ctx.readConfig("reader.json")?.accounts?.[0]?.books?.find((entry) => entry.itemId === book.Id) ?? null
  const shownPage = async () => Number(/Page (\d+) of/.exec(await ctx.app.evaluate(footer))?.[1]) - 1
  const ready = `document.querySelector('[data-reader-book="${book.Id}"]')?.dataset.readerPhase === "ready"`
  const turn = async () => {
    const before = await shownPage()
    await ctx.key("ArrowRight", { code: "ArrowRight", keyCode: 39 })
    await ctx.until(async () => (await shownPage()) === before + 1, `a turn to page ${before + 2}`, 20000)
    return before + 1
  }
  const read = async (description) => {
    await ctx.until(`document.activeElement?.getAttribute("href")?.startsWith("/read/")`, `${description}: the page focuses its read action`, 20000)
    await enter(ctx)
  }
  const leave = async () => {
    await ctx.key("Escape")
    await ctx.until(async () => (await ctx.pathname()) === `/item/${book.Id}`, "Escape leaves the reader")
  }
  const writes = () => proxy.stats.answered.filter((entry) => entry.method === "POST" && entry.path.endsWith(writePath)).length

  await ctx.press({ role: "link", name: "Books" })
  await ctx.press({ role: "link", name: `Open details for ${BOOK}` })
  await read("first open")
  await ctx.until(ready, "the reader to open the book", 120000)
  report.firstOpen = await shownPage()
  check(report.firstOpen === SERVER_PAGE, "a book only Jellyfin has a place for opens there", report.firstOpen)

  refuse = true
  await turn()
  const kept = await turn()
  await ctx.until(() => writes() >= 1 && place()?.location?.page === kept, "the page read to be kept here after Jellyfin refused it", 15000)
  report.keptLocally = place()
  await leave()
  await read("reopen after a refused write")
  await ctx.until(ready, "the reader to open the book again", 60000)
  report.reopenedAfterRefusal = await shownPage()
  check(report.reopenedAfterRefusal === kept, "the place Jellyfin refused wins over Jellyfin's older one while Jellyfin still holds that", { kept, opened: report.reopenedAfterRefusal, local: report.keptLocally })
  await ctx.screenshot("01-reopened-after-refusal")
  refuse = false

  const synced = await turn()
  await ctx.until(() => place()?.location?.page === synced && place()?.syncedTicks === synced * 1e4, "a turn to reach Jellyfin again", 15000)
  const gate = deferred()
  holdNext = gate
  const heldPage = await turn()
  await ctx.until(() => proxy.stats.held.some((entry) => entry.path.endsWith(writePath) && !entry.releasedAt), `Jellyfin's write of page ${heldPage + 1} to be held`, 15000)
  const lastShown = await turn()
  await leave()
  await read("reopen while a write is held")
  const openedWhileHeld = await ctx.until(ready, "the reader to open while the write is held", 3000).then(() => true, () => false)
  report.held = { heldPage, lastShown, openedWhileHeld }
  if (!openedWhileHeld) gate.release()
  await ctx.until(ready, "the reader to open the book again", 60000)
  report.held.opened = await shownPage()
  check(report.held.opened === lastShown, "the reopened reader opens at the last page the closed one showed", report.held)
  let newest = report.held.opened
  for (let step = 0; step < 3; step += 1) newest = await turn()
  if (openedWhileHeld) {
    await ctx.until(() => place()?.location?.page === newest && place()?.syncedTicks === newest * 1e4, "the reopened reader's newest page to reach Jellyfin while the old write is held", 15000)
    gate.release()
  }
  let stableSince = Date.now()
  let last = ""
  await ctx.until(async () => {
    const server = (await ctx.jellyfin(`/Items/${book.Id}?userId=${ctx.account.userId}`)).UserData.PlaybackPositionTicks
    const now = JSON.stringify({ local: place(), server })
    if (now !== last) [last, stableSince] = [now, Date.now()]
    return Date.now() - stableSince > 4000
  }, "every write to land and hold still for 4 s", 40000)
  report.final = { newest, ...JSON.parse(last) }
  check(report.final.local?.location?.page === newest, "the app keeps the newest page read", report.final)
  check(report.final.server === newest * 1e4, "Jellyfin holds the newest page read", report.final)
  check(report.final.local?.location?.pages === PAGES, `the place counts all ${PAGES} pages`)
  await ctx.screenshot("02-after-reopen")
  await leave()

  ctx.writeEvidence("book-progress.json", JSON.stringify(report, null, 2))
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} book-progress checks failed: ${failed.map((entry) => entry.message).join("; ")}`)
}
