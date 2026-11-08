import { execFileSync } from "node:child_process"

// The reader on a 4K screen and by mouse alone (`--account`, Linux, with
// MEDIAFLICK_VERIFY_SCREEN=3840x2160x24): the text size and column width the
// reader's size gives, real X11 left and right clicks on the page, a drag
// that selects text, the native menu only outside the book, and the same
// clicks on a fixed-layout book. Writes reader-mouse.json. See features/books.md.
const EPUB = "Pride and Prejudice"
const FIXED = "MediaFlick Fixed Layout"
const SCREEN = { width: 3840, height: 2160 }
const SMALL = { width: 1920, height: 1080 }
const FOOTER_PX = 32

const phase = (id) => `document.querySelector('[data-reader-book="${id}"]')?.dataset.readerPhase`
const layer = `document.querySelector("[data-reader-layer]")?.dataset.readerLayer`
const footerText = `[...document.querySelectorAll("[data-reader-footer] > span")].map((span) => span.innerText).join(" · ")`
const x = (...args) => execFileSync("xdotool", args.map(String), { encoding: "utf8" })
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
/**
 * A chapter's paragraph with no link, which a click would follow: Project
 * Gutenberg's own header and footer fix their text size in the book's CSS.
 */
const bodyParagraph = `(p) => p.textContent.trim().length > 200 && !p.closest("[id^='pg-']") && !p.querySelector("a")`

/** The reader's own rule (ui/src/reader/appearance.ts), restated as the oracle. */
function expectedBase(width, height) {
  return Math.max(16, Math.round(24 * Math.min(width / 1920, height / 1048) * 2) / 2)
}

function appWindow() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) .*?(\d+)x(\d+)\+/gm)]
    .map(([, id, width, height]) => ({ id, area: Number(width) * Number(height) }))
    .sort((a, b) => b.area - a.area)[0].id
}

/** Top-level windows that are viewable now; a native menu is one more of them. */
function viewableTopLevels() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) /gm)].flatMap(([, id]) => {
    const info = execFileSync("xwininfo", ["-id", id], { encoding: "utf8" })
    if (!/Map State: IsViewable/.test(info)) return []
    const size = /Width: (\d+)[\s\S]*Height: (\d+)/.exec(info)
    return [{ id, width: Number(size[1]), height: Number(size[2]), overrideRedirect: /Override Redirect State: yes/.test(info) }]
  })
}

function place(ctx, id) {
  return ctx.readConfig("reader.json")?.accounts?.[0]?.books?.find((book) => book.itemId === id)?.location ?? null
}

async function enter(ctx) {
  const event = { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 }
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyDown", text: "\r", unmodifiedText: "\r", ...event })
  await ctx.app.send("Input.dispatchKeyEvent", { type: "keyUp", ...event })
}

async function open(ctx, id, name) {
  await ctx.press({ role: "link", name: "Books" })
  await ctx.press({ role: "link", name: `Open details for ${name}` })
  await ctx.until(`document.activeElement?.getAttribute("href")?.startsWith("/read/")`, `${name}: the page focuses its read action`)
  await enter(ctx)
  await ctx.until(`${phase(id)} === "ready"`, `${name}: the reader opens`, 120000)
  return ctx.until(() => place(ctx, id), `${name}: the opened place saved`, 15000)
}

async function changedFrom(ctx, id, previous, description) {
  const before = JSON.stringify(previous)
  return ctx.until(() => {
    const now = place(ctx, id)
    return now && JSON.stringify(now) !== before && now
  }, description, 15000)
}

/** The place and the footer once nothing has moved for the time the reader takes to save twice. */
async function unchanged(ctx, id, previous, footer) {
  await pause(2500)
  return { place: place(ctx, id), footer: await ctx.app.evaluate(footerText), same: JSON.stringify(place(ctx, id)) === JSON.stringify(previous) && (await ctx.app.evaluate(footerText)) === footer }
}

async function resize(ctx, window, { width, height }) {
  x("windowmove", window, 0, 0)
  x("windowsize", window, width, height)
  return ctx.until(async () => {
    const size = await ctx.app.evaluate(`({ width: innerWidth, height: innerHeight })`)
    return size.width === width && size.height === height && size
  }, `the page to be ${width}x${height}`, 15000)
}

/** Sizes from inside the section document, as the reader laid it out. */
const measureText = `(() => {
  const view = document.querySelector("foliate-view")
  const host = view.parentElement
  const { doc } = view.renderer.getContents()[0]
  const paragraph = [...doc.querySelectorAll("p")].find(${bodyParagraph})
  return {
    host: { width: host.clientWidth, height: host.clientHeight },
    htmlFontPx: parseFloat(doc.defaultView.getComputedStyle(doc.documentElement).fontSize),
    paragraphFontPx: parseFloat(doc.defaultView.getComputedStyle(paragraph).fontSize),
    paragraphClass: paragraph.className,
    columnPx: parseFloat(doc.defaultView.getComputedStyle(doc.documentElement).columnWidth),
    maxInlineSize: view.renderer.getAttribute("max-inline-size"),
    margin: view.renderer.getAttribute("margin"),
  }
})()`

/** A point in the window on a line of visible book text, and a second one further along that line. */
const textPoint = `(() => {
  const view = document.querySelector("foliate-view")
  const bounds = view.getBoundingClientRect()
  for (const { doc } of view.renderer.getContents()) {
    const frame = doc.defaultView.frameElement.getBoundingClientRect()
    for (const node of doc.querySelectorAll("p")) {
      if (!(${bodyParagraph})(node)) continue
      const range = doc.createRange()
      range.selectNodeContents(node)
      for (const line of range.getClientRects()) {
        const left = frame.left + line.left, top = frame.top + line.top
        if (line.width > 300 && left > bounds.left + 50 && left + line.width < bounds.right - 50 && top > bounds.top + 100 && top + line.height < bounds.bottom - 100)
          return { x: Math.round(left + 20), y: Math.round(top + line.height / 2), toX: Math.round(left + line.width - 20) }
      }
    }
  }
  return null
})()`

const fixedPagePoint = `(() => {
  const view = document.querySelector("foliate-view")
  const { doc } = view.renderer.getContents()[0]
  const frame = doc.defaultView.frameElement.getBoundingClientRect()
  return { x: Math.round(frame.left + frame.width / 2), y: Math.round(frame.top + frame.height / 2) }
})()`

export default async function (ctx) {
  const report = { steps: {}, checks: [] }
  const check = (ok, message, detail) => {
    report.checks.push({ ok, message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}`)
  }
  try {
    if (process.platform !== "linux") throw new Error("reader-mouse sends real X11 input and runs on Linux only")
    await ctx.signIn({})
    const ids = Object.fromEntries((await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&Recursive=true&IncludeItemTypes=Book`)).Items.map((item) => [item.Name, item.Id]))
    const epub = ids[EPUB]
    const fixed = ids[FIXED]
    if (!epub || !fixed) throw new Error(`fixtures missing: ${EPUB} ${Boolean(epub)}, ${FIXED} ${Boolean(fixed)}`)
    ctx.restoreItemAfterRun(epub)
    ctx.restoreItemAfterRun(fixed)

    const window = appWindow()
    report.window = window
    report.screen = await resize(ctx, window, SCREEN)
    check(report.screen.width === SCREEN.width && report.screen.height === SCREEN.height, `the app fills the ${SCREEN.width}x${SCREEN.height} screen`, report.screen)

    await open(ctx, epub, EPUB)
    for (let chapter = 0; chapter < 8 && !(await ctx.app.evaluate(textPoint)); chapter += 1) {
      await ctx.key("]", { code: "BracketRight", keyCode: 221 })
      await pause(1000)
    }
    report.appearance = ctx.readConfig("reader.json")?.accounts?.[0]?.appearance ?? null
    const fontSize = report.appearance?.fontSize ?? 100
    check(fontSize === 100, "the reader's text size setting is 100%", report.appearance)

    const sizes = {}
    for (const [label, size] of [["4k", SCREEN], ["1080p", SMALL]]) {
      if (label !== "4k") await resize(ctx, window, size)
      const base = expectedBase(size.width, size.height - FOOTER_PX)
      const measured = await ctx.until(async () => {
        const now = await ctx.app.evaluate(measureText)
        return now.htmlFontPx === (base * fontSize) / 100 && now.maxInlineSize === `${34 * base}px` && now
      }, `${label}: the book laid out at a ${base}px base`, 15000).catch(async () => ctx.app.evaluate(measureText))
      sizes[label] = { expectedBasePx: base, expectedColumnMaxPx: 34 * base, ...measured, columnEm: measured.columnPx / measured.htmlFontPx }
      check(measured.htmlFontPx === base, `${label}: text is ${base}px (measured ${measured.htmlFontPx}px; a body paragraph ${measured.paragraphFontPx}px)`, sizes[label])
      check(measured.maxInlineSize === `${34 * base}px` && measured.margin === `${2 * base}px`, `${label}: the column is at most 34em (${measured.maxInlineSize}) with ${measured.margin} above and below`)
      check(measured.columnPx > 0 && measured.columnPx <= 34 * base, `${label}: a laid-out column is ${measured.columnPx}px, ${sizes[label].columnEm.toFixed(2)}em`)
      ctx.displayCapture(`0${label === "4k" ? 1 : 2}-reading-${label}`)
    }
    check(Math.abs(sizes["4k"].columnEm - sizes["1080p"].columnEm) < 0.05, `a column holds the same em at both sizes (${sizes["4k"].columnEm.toFixed(2)} and ${sizes["1080p"].columnEm.toFixed(2)})`)
    check(sizes["4k"].paragraphFontPx === 2 * sizes["1080p"].paragraphFontPx, `a body paragraph doubles from 1080p to 4K (${sizes["1080p"].paragraphFontPx}px to ${sizes["4k"].paragraphFontPx}px)`)
    report.sizes = sizes
    await resize(ctx, window, SCREEN)
    await ctx.until(async () => (await ctx.app.evaluate(measureText)).htmlFontPx === sizes["4k"].expectedBasePx, "the text grows back with the window", 15000)

    x("mousemove", "--window", window, 3820, 20)
    x("windowfocus", "--sync", window)
    const start = await ctx.until(async () => {
      await pause(300)
      return place(ctx, epub)
    }, "the place to hold after the resize", 15000)
    const point = await ctx.until(textPoint, "a line of book text to click", 10000)
    report.steps.click = { point, before: start, footerBefore: await ctx.app.evaluate(footerText) }
    x("mousemove", "--window", window, point.x, point.y, "click", 1)
    const forward = await changedFrom(ctx, epub, start, "a left click on the text turns the page").catch(() => null)
    report.steps.click.after = forward
    report.steps.click.footerAfter = await ctx.app.evaluate(footerText)
    check(Boolean(forward) && forward.fraction > start.fraction, `a left click on the text moves the place forward (${start.fraction} to ${forward?.fraction})`)

    const menusBefore = viewableTopLevels()
    x("mousemove", "--window", window, point.x, point.y, "click", 3)
    const backward = forward && (await changedFrom(ctx, epub, forward, "a right click on the text turns back").catch(() => null))
    await pause(500)
    const menusAfter = viewableTopLevels()
    report.steps.rightClick = { before: forward, after: backward, footerAfter: await ctx.app.evaluate(footerText), menusBefore, menusAfter }
    check(JSON.stringify(backward) === JSON.stringify(start), `a right click on the text returns to the place before the click (${backward?.fraction})`)
    check(menusAfter.length === menusBefore.length, `no native menu on the book (${menusBefore.length} viewable top-level windows before, ${menusAfter.length} after)`)
    ctx.displayCapture("03-after-right-click")

    const line = await ctx.until(textPoint, "a line of book text to drag across", 10000)
    const footerBeforeDrag = await ctx.app.evaluate(footerText)
    x("mousemove", "--window", window, line.x, line.y, "mousedown", 1)
    for (const step of [0.25, 0.5, 0.75, 1]) x("mousemove", "--window", window, Math.round(line.x + (line.toX - line.x) * step), line.y)
    x("mouseup", 1)
    const selected = await ctx.until(`document.querySelector("foliate-view").renderer.getContents().map(({ doc }) => doc.getSelection().toString()).join("").trim()`, "the drag selects text", 5000).catch(() => "")
    const afterDrag = await unchanged(ctx, epub, backward, footerBeforeDrag)
    report.steps.drag = { line, selected, ...afterDrag }
    check(selected.length > 10, `a drag across a line selects its text (${JSON.stringify(selected.slice(0, 60))})`)
    check(afterDrag.same, "the drag leaves the place where it was")
    ctx.displayCapture("04-drag-selected")

    const footer = await ctx.app.evaluate(`(() => { const r = document.querySelector("[data-reader-footer] > span").getBoundingClientRect(); return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) } })()`)
    const footerMenusBefore = viewableTopLevels()
    x("mousemove", "--window", window, footer.x, footer.y, "click", 3)
    const shown = await ctx.until(() => {
      const now = viewableTopLevels()
      return now.length > footerMenusBefore.length && now.filter((w) => !footerMenusBefore.some((b) => b.id === w.id))
    }, "a right click on the footer opens the native menu", 10000).catch(() => null)
    ctx.displayCapture("05-footer-menu")
    // Under Xvfb no window manager hands the menu the keyboard, so Escape can reach the reader too and close the book; a click outside the menu closes it instead.
    x("mousemove", "--window", window, footer.x, footer.y, "click", 1)
    const closed = await ctx.until(() => viewableTopLevels().length === footerMenusBefore.length, "a click on the footer closes the native menu", 10000).then(() => true, () => false)
    const afterFooter = await unchanged(ctx, epub, backward, footerBeforeDrag)
    report.steps.footer = { point: footer, menusBefore: footerMenusBefore, menuWindows: shown, menusAfterClose: viewableTopLevels(), closed, path: await ctx.pathname(), ...afterFooter }
    check(Boolean(shown), `a right click on the footer shows the native menu (${footerMenusBefore.length} viewable top-level windows before, ${footerMenusBefore.length + (shown?.length ?? 0)} with it)`, shown)
    check(closed, "a click outside it closes it")
    check(afterFooter.same && report.steps.footer.path === `/read/${epub}`, "the footer's menu leaves the book and its place as they were")

    const selection = `document.querySelector("foliate-view").renderer.getContents().map(({ doc }) => doc.getSelection().toString()).join("")`
    const selectedBefore = await ctx.app.evaluate(selection)
    x("mousemove", "--window", window, Math.round((line.x + line.toX) / 2), line.y, "click", 1)
    const dismissed = await unchanged(ctx, epub, backward, footerBeforeDrag)
    const selectedAfter = await ctx.app.evaluate(selection)
    report.steps.dismiss = { selectedBefore, selectedAfter, ...dismissed }
    check(selectedBefore.length > 0 && selectedAfter === "" && dismissed.same, "a click on selected text clears the selection and does not turn")
    x("mousemove", "--window", window, point.x, point.y, "click", 1)
    const focused = await changedFrom(ctx, epub, backward, "the next click turns the page").catch(() => null)
    const footerBeforeMenu = await ctx.app.evaluate(footerText)
    x("key", "Menu")
    const menuOpened = await ctx.until(`${layer} === "menu"`, "the Menu key opens the reader's menu", 10000).then(() => true, () => false)
    const afterMenuKey = await unchanged(ctx, epub, focused, footerBeforeMenu)
    report.steps.menuKey = { focused, menuOpened, menuWindows: viewableTopLevels().length, ...afterMenuKey }
    check(menuOpened && afterMenuKey.same, "the Menu key opens the reader's menu without turning a page")
    x("key", "XF86Back")
    await ctx.until(`${layer} === "reading"`, "Back closes the menu")
    x("key", "XF86Back")
    await ctx.until(async () => (await ctx.pathname()) === `/item/${epub}`, "Back leaves the reader")

    const shownPage = async () => Number(/Page (\d+) of \d+/.exec(await ctx.app.evaluate(footerText))?.[1])
    const fixedStart = await open(ctx, fixed, FIXED)
    const pageStart = await shownPage()
    const page = await ctx.app.evaluate(fixedPagePoint)
    x("mousemove", "--window", window, page.x, page.y, "click", 1)
    const fixedForward = await changedFrom(ctx, fixed, fixedStart, "a left click on the fixed page turns it").catch(() => null)
    const pageForward = await shownPage()
    x("mousemove", "--window", window, page.x, page.y, "click", 3)
    const fixedBack = fixedForward && (await changedFrom(ctx, fixed, fixedForward, "a right click on the fixed page turns back").catch(() => null))
    const pageBack = await shownPage()
    report.steps.fixed = { point: page, before: fixedStart, forward: fixedForward, back: fixedBack, pages: [pageStart, pageForward, pageBack], footer: await ctx.app.evaluate(footerText) }
    check(Boolean(fixedForward) && pageForward === pageStart + 1, `a left click moves the fixed-layout book one page forward (page ${pageStart} to ${pageForward})`)
    check(Boolean(fixedBack) && pageBack === pageStart && JSON.stringify(fixedBack) === JSON.stringify(fixedStart), `a right click moves it back (page ${pageBack})`)
    ctx.displayCapture("06-fixed-layout")
    x("key", "XF86Back")
    await ctx.until(async () => (await ctx.pathname()) === `/item/${fixed}`, "Back leaves the fixed-layout book")
  } finally {
    ctx.writeEvidence("reader-mouse.json", JSON.stringify(report, null, 2))
  }
  const failed = report.checks.filter((entry) => !entry.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.map((entry) => entry.message).join("; ")}`)
}
