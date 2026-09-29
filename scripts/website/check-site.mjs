// End-to-end check of flick.media as a visitor's browser sees it.
//
//   node scripts/website/check-site.mjs [output-dir]
//
// Serves website/public with the Content-Security-Policy from its _headers
// file, drives headless Chrome (or Edge) over CDP at a desktop and a phone
// viewport, and exercises the page: the section order (Highlights and the
// Companion before the standard features), scrolling every section into
// view, the hero's readouts, the Browse walkthrough step by step, the player
// switch, the Companion tabs, the accent swatches and the gallery lightbox. It fails
// on console errors, CSP violations, missing assets, horizontal overflow or
// broken interactions.
//
// The output directory (default build/website-check) receives report.json,
// full-page screenshots (desktop.png and mobile.png) and one viewport
// screenshot per walkthrough step (walk-<viewport>-<step>.png).

import { createServer } from "node:http"
import { readFile, stat, mkdir, writeFile, rm } from "node:fs/promises"
import { existsSync } from "node:fs"
import { spawn } from "node:child_process"
import { tmpdir } from "node:os"
import path from "node:path"
import { connect, sleep } from "./cdp.mjs"

const root = path.resolve(import.meta.dirname, "../..")
const siteDir = path.join(root, "website", "public")
const outDir = path.resolve(process.argv[2] ?? path.join(root, "build", "website-check"))
const CDP_PORT = Number(process.env.CDP_PORT ?? 9351)

const TYPES = {
  ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".svg": "image/svg+xml",
  ".webp": "image/webp", ".mp4": "video/mp4", ".webm": "video/webm", ".xml": "application/xml", ".txt": "text/plain",
}

// Only the site-wide block of _headers matters here: the CSP is what can
// break the page.
const headersFile = await readFile(path.join(siteDir, "_headers"), "utf8")
const csp = headersFile.match(/^\s*Content-Security-Policy:\s*(.+)$/m)?.[1]
if (!csp) throw new Error("website/public/_headers has no Content-Security-Policy")

const requests = []
const server = createServer(async (request, response) => {
  let pathname = decodeURIComponent(new URL(request.url ?? "/", "http://site").pathname)
  if (pathname.endsWith("/")) pathname += "index.html"
  const file = path.join(siteDir, pathname)
  if (!file.startsWith(siteDir)) {
    response.writeHead(403).end()
    return
  }
  try {
    const info = await stat(file)
    const data = await readFile(file)
    const headers = { "Content-Type": TYPES[path.extname(file)] ?? "application/octet-stream", "Content-Security-Policy": csp, "Accept-Ranges": "bytes" }
    const range = /bytes=(\d+)-(\d*)/.exec(request.headers.range ?? "")
    requests.push({ path: pathname, status: range ? 206 : 200 })
    if (range) {
      const start = Number(range[1])
      const end = range[2] ? Number(range[2]) : info.size - 1
      response.writeHead(206, { ...headers, "Content-Range": `bytes ${start}-${end}/${info.size}`, "Content-Length": end - start + 1 })
      response.end(data.subarray(start, end + 1))
    } else {
      response.writeHead(200, { ...headers, "Content-Length": info.size })
      response.end(data)
    }
  } catch {
    requests.push({ path: pathname, status: 404 })
    response.writeHead(404).end("not found")
  }
})
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve))
const address = server.address()
if (typeof address !== "object" || !address) throw new Error("static server has no address")
const siteUrl = `http://127.0.0.1:${address.port}/`

const browserPath = [
  process.env.CHROME_PATH,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
].find((candidate) => candidate && existsSync(candidate))
if (!browserPath) throw new Error("no Chrome or Edge found; set CHROME_PATH")

const profileDir = path.join(tmpdir(), `mediaflick-site-check-${process.pid}`)
const browser = spawn(browserPath, [
  "--headless=new", `--remote-debugging-port=${CDP_PORT}`, `--user-data-dir=${profileDir}`,
  "--no-first-run", "--no-default-browser-check", "--autoplay-policy=no-user-gesture-required",
  "--hide-scrollbars", "--mute-audio", "about:blank",
], { stdio: "ignore" })

const report = { site: siteUrl, browser: path.basename(browserPath), checkedAt: new Date().toISOString(), viewports: {}, failures: [] }
const fail = (viewport, message) => report.failures.push(`${viewport}: ${message}`)

let page
try {
  page = await connect(CDP_PORT, (target) => target.url === "about:blank" || target.url.startsWith(siteUrl))
  const problems = []
  page.on("Runtime.consoleAPICalled", (event) => {
    if (event.type === "error" || event.type === "assert") problems.push(`console.${event.type}: ${event.args.map((arg) => arg.value ?? arg.description).join(" ")}`)
  })
  page.on("Runtime.exceptionThrown", (event) => problems.push(`exception: ${event.exceptionDetails.exception?.description ?? event.exceptionDetails.text}`))
  page.on("Log.entryAdded", ({ entry }) => {
    if (entry.level === "error") problems.push(`log.${entry.source}: ${entry.text}${entry.url ? ` (${entry.url})` : ""}`)
  })
  await page.send("Runtime.enable")
  await page.send("Log.enable")
  await page.send("Page.enable")
  // CSP violations are reported to the page itself; collect them there.
  await page.send("Page.addScriptToEvaluateOnNewDocument", {
    source: `window.__cspViolations = []; document.addEventListener("securitypolicyviolation", (event) => window.__cspViolations.push(event.violatedDirective + " " + event.blockedURI))`,
  })

  const viewports = {
    desktop: { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false },
    mobile: { width: 390, height: 844, deviceScaleFactor: 2, mobile: true },
  }

  for (const [name, metrics] of Object.entries(viewports)) {
    problems.length = 0
    await page.send("Emulation.setDeviceMetricsOverride", metrics)
    await page.send("Emulation.setTouchEmulationEnabled", { enabled: metrics.mobile })
    const loaded = new Promise((resolve) => {
      const off = page.on("Page.loadEventFired", () => {
        off()
        resolve()
      })
    })
    await page.send("Page.navigate", { url: siteUrl })
    await loaded
    await sleep(800)

    const heroTop = await page.evaluate(`document.querySelector(".screen-hero").getBoundingClientRect().top`)
    if (name === "desktop" && heroTop > metrics.height - 120) fail(name, `hero window starts at ${Math.round(heroTop)}px, below the fold`)

    // The headline's words have risen and the readouts follow the recording:
    // the library ones while it tours, the playback ones once the film starts.
    const hero = await page.evaluate(`(async () => {
      const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
      const stage = document.querySelector("[data-hero-stage]")
      const video = document.querySelector("[data-hero]")
      const words = [...document.querySelectorAll("h1 .w")]
      await wait(1200)
      const risen = words.every((word) => getComputedStyle(word).opacity === "1" && getComputedStyle(word).translate === "none")
      const browse = { phase: stage.dataset.phase, title: stage.querySelector("[data-hero-title]").textContent, playChip: getComputedStyle(stage.querySelector(".chip-play")).opacity, browseChip: getComputedStyle(stage.querySelector(".chip-browse")).opacity }
      video.currentTime = Number(video.dataset.playAt) + 0.5
      await wait(1600)
      const play = { phase: stage.dataset.phase, title: stage.querySelector("[data-hero-title]").textContent, playChip: getComputedStyle(stage.querySelector(".chip-play")).opacity, browseChip: getComputedStyle(stage.querySelector(".chip-browse")).opacity }
      video.currentTime = 0
      // Headless Chrome may never start the recording; a pending play() is
      // recorded as such rather than waited for.
      const playback = await Promise.race([video.play().then(() => "playing", (error) => String(error)), wait(1500).then(() => "pending")])
      return { risen, playing: !video.paused, playback, readyState: video.readyState, browse, play }
    })()`)
    if (!hero.risen) fail(name, "headline words did not rise into place")
    if (hero.playing) {
      if (hero.browse.phase !== "browse" || hero.browse.browseChip !== "1" || hero.browse.playChip !== "0") fail(name, `hero readouts did not follow the library tour: ${JSON.stringify(hero.browse)}`)
    }
    if (hero.play.phase !== "play" || hero.play.playChip !== "1" || hero.play.browseChip !== "0" || !hero.play.title.includes("Now playing")) fail(name, `hero readouts did not follow playback: ${JSON.stringify(hero.play)}`)

    // Walk the page so every reveal, lazy image and in-view video fires.
    const height = await page.evaluate("document.documentElement.scrollHeight")
    for (let y = 0; y < height; y += Math.round(metrics.height * 0.6)) {
      await page.evaluate(`scrollTo(0, ${y})`)
      await sleep(180)
    }
    await sleep(900)

    const state = await page.evaluate(`(() => {
      const hidden = [...document.querySelectorAll(".reveal:not(.is-visible)")].map((element) => element.className)
      // Lazy images in hidden tabs or off-screen gallery slots never load on
      // their own; only those that tried and failed count here.
      const brokenImages = [...document.images].filter((image) => image.complete && image.currentSrc && image.naturalWidth === 0).map((image) => image.currentSrc)
      const overflow = document.documentElement.scrollWidth - innerWidth
      return { hidden, brokenImages, overflow, csp: window.__cspViolations }
    })()`)
    if (state.hidden.length) fail(name, `${state.hidden.length} sections never revealed: ${state.hidden.join(", ")}`)

    // What sets the app apart comes first: Highlights straight after the
    // hero, then the Companion, and only then the standard features.
    const order = await page.evaluate(`(() => {
      const sections = [...document.querySelectorAll("main > section[id]")].map((section) => section.id)
      const cards = [...document.querySelectorAll("#highlights .feature")].map((card) => ({
        title: card.querySelector("h3")?.textContent ?? "",
        tag: card.querySelector(".feature-tag")?.textContent ?? "",
        height: card.getBoundingClientRect().height,
      }))
      return { sections, cards }
    })()`)
    const at = (id) => order.sections.indexOf(id)
    if (order.sections[1] !== "highlights") fail(name, `the first section after the hero is ${order.sections[1]}, not highlights`)
    if (!(at("highlights") < at("companion") && at("companion") < at("players") && at("companion") < at("sync"))) fail(name, `sections are out of order: ${order.sections.join(", ")}`)
    if (order.cards.length < 6) fail(name, `Highlights lists ${order.cards.length} features`)
    for (const card of order.cards) if (!card.tag || !card.title || card.height < 200) fail(name, `Highlights card is incomplete: ${JSON.stringify(card)}`)
    if (state.brokenImages.length) fail(name, `images failed: ${state.brokenImages.join(", ")}`)

    // Every media URL the page references must decode, loaded or not yet.
    const assets = await page.evaluate(`(async () => {
      const urls = new Set()
      for (const image of document.images) {
        urls.add(image.src)
        for (const candidate of (image.getAttribute("srcset") ?? "").split(",")) if (candidate.trim()) urls.add(new URL(candidate.trim().split(" ")[0], location.href).href)
      }
      for (const element of document.querySelectorAll("[data-full]")) urls.add(new URL(element.dataset.full, location.href).href)
      for (const video of document.querySelectorAll("video[poster]")) urls.add(video.poster)
      const failed = []
      for (const url of [...urls].filter((url) => url.startsWith(location.origin))) {
        const image = new Image()
        image.src = url
        try { await image.decode() } catch { failed.push(url) }
      }
      return { checked: urls.size, failed }
    })()`)
    if (assets.failed.length) fail(name, `images do not decode: ${assets.failed.join(", ")}`)
    if (state.overflow > 1) fail(name, `page overflows horizontally by ${state.overflow}px`)
    if (state.csp.length) fail(name, `CSP violations: ${state.csp.join("; ")}`)

    // The walkthrough: bring each step to the middle of the viewport and
    // expect the pinned window to show that step's frame, name it in its
    // title bar, and play the hover recording only on the hover step. A
    // screenshot of each step is kept as the artifact.
    await mkdir(outDir, { recursive: true })
    const walkthrough = []
    const stepNames = await page.evaluate(`[...document.querySelectorAll("[data-walk-step]")].map((step) => step.dataset.walkStep)`)
    for (const step of stepNames) {
      const result = await page.evaluate(`(async () => {
        const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
        const walk = document.querySelector("[data-walk]")
        const step = walk.querySelector('[data-walk-step="${step}"]')
        const rect = step.getBoundingClientRect()
        // Where a reader would stop: the step in the middle beside the window,
        // or, on a phone, its text just under the pinned window.
        const stage = walk.querySelector(".walk-stage")
        const pinnedBottom = parseFloat(getComputedStyle(stage).top) + stage.offsetHeight
        const stacked = ${metrics.mobile}
        scrollTo({ top: scrollY + rect.top - (stacked ? pinnedBottom + 8 : innerHeight / 2 - rect.height / 2), behavior: "instant" })
        await wait(1000)
        const frame = walk.querySelector('[data-walk-frame="${step}"]')
        const stageRect = stage.getBoundingClientRect()
        const others = [...walk.querySelectorAll(".walk-frame")].filter((candidate) => candidate !== frame).map((candidate) => getComputedStyle(candidate).opacity)
        return {
          active: walk.dataset.walkActive,
          frameOpacity: getComputedStyle(frame).opacity,
          othersHidden: others.every((opacity) => opacity === "0"),
          title: walk.querySelector("[data-walk-title]").textContent,
          expectedTitle: step.dataset.walkTitle,
          stepLit: getComputedStyle(step).opacity === "1",
          stagePinned: stageRect.top >= 0 && stageRect.bottom <= innerHeight + 1,
          videoPlaying: !walk.querySelector("video").paused,
        }
      })()`)
      const { data } = await page.send("Page.captureScreenshot", { format: "png" })
      const shot = path.join(outDir, `walk-${name}-${step}.png`)
      await writeFile(shot, Buffer.from(data, "base64"))
      walkthrough.push({ step, ...result, screenshot: path.relative(root, shot) })
      if (result.active !== step) fail(name, `walkthrough step ${step} did not become active (active: ${result.active})`)
      if (result.frameOpacity !== "1" || !result.othersHidden) fail(name, `walkthrough window does not show the ${step} frame alone`)
      if (result.title !== result.expectedTitle) fail(name, `walkthrough title is "${result.title}" on step ${step}`)
      if (!result.stepLit) fail(name, `walkthrough step ${step} is not highlighted`)
      if (!result.stagePinned) fail(name, `walkthrough window is not pinned in view on step ${step}`)
      if (result.videoPlaying !== (step === "hover")) fail(name, `hover recording ${result.videoPlaying ? "plays" : "does not play"} on step ${step}`)
    }

    // The current section is tracked in the nav's sliding underline and, on
    // wide screens, in the spine.
    const tracking = await page.evaluate(`(async () => {
      const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
      const target = document.querySelector("#companion")
      scrollTo({ top: target.offsetTop + innerHeight * 0.3, behavior: "instant" })
      await wait(500)
      const nav = document.querySelector(".chrome-nav")
      const spine = document.querySelector(".spine")
      return {
        navVisible: getComputedStyle(nav).display !== "none",
        navCurrent: nav.querySelector("a[aria-current]")?.getAttribute("href"),
        ink: nav.hasAttribute("data-ink") && getComputedStyle(nav.querySelector(".nav-ink")).opacity === "1",
        inkWidth: parseFloat(nav.style.getPropertyValue("--ink-w")) || 0,
        spineVisible: getComputedStyle(spine).display !== "none",
        spineCurrent: spine.querySelector("a[aria-current]")?.dataset.name,
        spineFill: getComputedStyle(spine.querySelector(".spine-line i")).scale,
      }
    })()`)
    if (tracking.navVisible && (tracking.navCurrent !== "#companion" || !tracking.ink || tracking.inkWidth <= 0)) fail(name, `nav did not track the Companion section: ${JSON.stringify(tracking)}`)
    if (metrics.width >= 1400 && (!tracking.spineVisible || tracking.spineCurrent !== "Companion")) fail(name, `spine did not track the Companion section: ${JSON.stringify(tracking)}`)

    // Interactions, through the page's own controls.
    const interactions = await page.evaluate(`(async () => {
      const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
      const results = {}
      document.querySelector('[data-switch-tab="external"]').click()
      await wait(200)
      results.playerSwitch = !document.querySelector('[data-switch-panel="external"]').hidden && document.querySelector('[data-switch-panel="builtin"]').hidden
      document.querySelector('[data-switch-tab="builtin"]').click()
      document.querySelector('[data-show="discover"]').click()
      await wait(200)
      results.showcaseTabs = !document.querySelector('[data-show-panel="discover"]').hidden
      const before = getComputedStyle(document.documentElement).getPropertyValue("--primary").trim()
      document.querySelector('[data-accent-choice="amber"]').click()
      const after = getComputedStyle(document.documentElement).getPropertyValue("--primary").trim()
      document.querySelector('[data-accent-choice="signal"]').click()
      results.accent = before !== after && after === "#ffbd4a"
      // Opening morphs the thumbnail into the dialog where view transitions
      // exist; closing through the dialog's own button morphs it back.
      document.querySelector("[data-full]").click()
      await wait(900)
      const image = document.querySelector("[data-lightbox-image]")
      await image.decode().catch(() => {})
      const lightbox = document.querySelector("[data-lightbox]")
      results.lightbox = lightbox.open && image.naturalWidth > 0
      lightbox.querySelector(".lightbox-close").click()
      await wait(900)
      results.lightboxClosed = !lightbox.open
      const videos = [...document.querySelectorAll("video")]
      await Promise.all(videos.map((video) => (video.readyState >= 1 ? null : (video.preload = "metadata", video.load(), new Promise((resolve) => { video.onloadedmetadata = resolve; video.onerror = resolve; setTimeout(resolve, 4000) })))))
      results.videos = videos.map((video) => ({ src: video.currentSrc, ok: video.readyState >= 1 && video.videoWidth > 0 }))
      return results
    })()`)
    for (const key of ["playerSwitch", "showcaseTabs", "accent", "lightbox", "lightboxClosed"]) {
      if (!interactions[key]) fail(name, `${key} did not work`)
    }
    for (const video of interactions.videos) if (!video.ok) fail(name, `video did not load: ${video.src}`)

    // Full-page screenshot from the top, with every section revealed.
    await page.evaluate("scrollTo(0, 0)")
    await sleep(900)
    const fullHeight = await page.evaluate("document.documentElement.scrollHeight")
    const { data } = await page.send("Page.captureScreenshot", {
      format: "png",
      captureBeyondViewport: true,
      // Chrome cannot rasterize past 16384 device pixels in one capture.
      clip: { x: 0, y: 0, width: metrics.width, height: fullHeight, scale: Math.min(1, 16000 / (fullHeight * metrics.deviceScaleFactor)) },
    })
    await mkdir(outDir, { recursive: true })
    const shot = path.join(outDir, `${name}.png`)
    await writeFile(shot, Buffer.from(data, "base64"))

    for (const problem of problems) fail(name, problem)
    report.viewports[name] = { ...metrics, heroTop: Math.round(heroTop), pageHeight: fullHeight, imagesChecked: assets.checked, hero, order, walkthrough, tracking, interactions, screenshot: path.relative(root, shot) }
  }
  report.notFound = [...new Set(requests.filter((entry) => entry.status === 404).map((entry) => entry.path))]
  for (const missing of report.notFound) report.failures.push(`404: ${missing}`)
} finally {
  page?.close()
  browser.kill()
  server.close()
  await sleep(500)
  await rm(profileDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }).catch(() => {})
}

await mkdir(outDir, { recursive: true })
await writeFile(path.join(outDir, "report.json"), JSON.stringify(report, null, 2))
console.log(`report: ${path.relative(root, path.join(outDir, "report.json"))}`)
if (report.failures.length) {
  console.error(report.failures.map((failure) => `  ✗ ${failure}`).join("\n"))
  process.exitCode = 1
} else {
  console.log("  ✓ desktop and mobile passed")
}
