// Captures the website's screenshots and screen recordings from the real app.
//
//   just build
//   node scripts/website/capture.mjs demo [scene ...]
//   node scripts/website/capture.mjs home [scene ...]
//   node scripts/website/encode.mjs
//
// `demo` signs in to Jellyfin's public demo server (public-domain and
// Creative Commons titles) in an empty profile. `home` runs a copy of the
// current user's profile for features that need the MediaFlick Companion:
// the release calendar, Release Timeline, ratings and requests. See
// launch.mjs for how the profiles are isolated.
//
// Raw PNG stills, JPEG screencast frames and overlay frames land in
// build/website-capture/raw together with report-<profile>.json, which
// records every scene, its route, its output files and any failure. Naming
// scenes re-captures only those and keeps the rest of the report. encode.mjs
// turns the raw files into the web assets under website/public/media. The
// capture app is closed at the end, and the script exits non-zero when a
// scene fails.

import { mkdir, rm, writeFile, readFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { spawnSync } from "node:child_process"
import path from "node:path"
import { connect, sleep } from "./cdp.mjs"
import { launch, stop, DEMO_URL } from "./launch.mjs"

const root = path.resolve(import.meta.dirname, "../..")
const rawDir = path.join(root, "build", "website-capture", "raw")
const VIEWPORT = { width: 1600, height: 1000 }
// Somewhere with no hover affordance. The sidebar expands under the pointer,
// so the top-right corner of the page is the quiet spot.
const PARK = { x: 1596, y: 4 }

const profile = process.argv[2]
const only = new Set(process.argv.slice(3))
if (!["demo", "home"].includes(profile)) throw new Error("usage: capture.mjs demo|home [scene ...]")

await mkdir(rawDir, { recursive: true })
const { port } = await launch(profile)
const app = await connect(port)
await app.send("Page.enable")
await app.send("Runtime.enable")

function windowScript(extra) {
  const args = ["-NoLogo", "-NoProfile", "-File", path.join(import.meta.dirname, "window.ps1"), "-Port", String(port), ...extra]
  const result = spawnSync("pwsh.exe", args, { encoding: "utf8" })
  if (result.status !== 0) throw new Error(`window.ps1 failed: ${result.stderr}`)
  return result.stdout
}

function sizeWindow(width, height) {
  return JSON.parse(windowScript(["-X", "40", "-Y", "40", "-Width", String(width), "-Height", String(height)]))
}

// CEF tiles screenshots taken under Emulation.setDeviceMetricsOverride, so
// the real window is sized until its CSS viewport matches, and captures run
// at the display's own device pixel ratio.
async function viewport(target = VIEWPORT) {
  const dpr = await app.evaluate("devicePixelRatio")
  const current = async () => app.evaluate("[innerWidth, innerHeight]")
  let [width, height] = await current()
  if (width === target.width && height === target.height) return dpr
  // window.ps1 works in physical pixels. Step by the scaled difference, then
  // one pixel at a time, because fractional scaling rounds unevenly.
  let size = JSON.parse(windowScript([]))
  for (let attempt = 0; attempt < 16; attempt++) {
    const stepX = Math.abs(target.width - width) > 1 ? Math.round((target.width - width) * dpr) : Math.sign(target.width - width)
    const stepY = Math.abs(target.height - height) > 1 ? Math.round((target.height - height) * dpr) : Math.sign(target.height - height)
    size = sizeWindow(size.width + stepX, size.height + stepY)
    await sleep(700)
    ;[width, height] = await current()
    if (width === target.width && height === target.height) return dpr
  }
  throw new Error("could not size the capture window to the viewport")
}

async function park() {
  await app.mouse("mouseMoved", PARK.x, PARK.y)
  await app.evaluate("document.activeElement?.blur?.(), true")
}

// Every visible image decoded and no loading placeholders left.
async function settled(extra = 700) {
  await app.waitFor(
    `(() => {
      const visible = (element) => {
        const rect = element.getBoundingClientRect()
        return rect.width > 0 && rect.bottom > 0 && rect.top < innerHeight && rect.right > 0 && rect.left < innerWidth
      }
      const images = [...document.images].filter(visible)
      return images.every((image) => image.complete && image.naturalWidth > 0)
        && ![...document.querySelectorAll('[data-slot="skeleton"], .animate-pulse')].some(visible)
    })()`,
    { timeout: 30000 },
  ).catch(() => console.warn("  images still loading; capturing anyway"))
  await sleep(extra)
}

async function go(route) {
  await app.route(route)
  await sleep(400)
  // Route changes keep the scroll container's position; every scene starts
  // at the top of its page.
  await app.evaluate(`(() => {
    for (const element of document.querySelectorAll("*")) if (element.scrollTop > 0) element.scrollTop = 0
    scrollTo(0, 0)
    return true
  })()`)
  await settled()
}

async function linkFor(text) {
  const href = await app.waitFor(
    `[...document.querySelectorAll("a[href^='/item/']")].find((link) => link.textContent.trim().startsWith(${JSON.stringify(text)}))?.getAttribute("href")`,
  )
  return href
}

// Full viewport only: any clip or scale also makes CEF tile the image.
// Cropping happens in encode.mjs.
async function still(name) {
  const { data } = await app.send("Page.captureScreenshot", { format: "png" })
  const file = path.join(rawDir, `${name}.png`)
  await writeFile(file, Buffer.from(data, "base64"))
  return file
}

// Screencast frames arrive only when the page repaints, each stamped with its
// own time, so the encoder turns them into a variable-duration sequence.
async function record(name, action, { tail = 800 } = {}) {
  const dir = path.join(rawDir, name)
  await rm(dir, { recursive: true, force: true })
  await mkdir(dir, { recursive: true })
  const frames = []
  const writes = []
  const off = app.on("Page.screencastFrame", (frame) => {
    const index = frames.length
    const file = `${String(index).padStart(5, "0")}.jpg`
    frames.push({ file, time: frame.metadata.timestamp })
    writes.push(writeFile(path.join(dir, file), Buffer.from(frame.data, "base64")))
    app.send("Page.screencastFrameAck", { sessionId: frame.sessionId }).catch(() => {})
  })
  await app.send("Page.startScreencast", { format: "jpeg", quality: 92, everyNthFrame: 1 })
  const started = Date.now() / 1000
  await sleep(300)
  await action()
  await sleep(tail)
  const stopped = Date.now() / 1000
  await app.send("Page.stopScreencast")
  off()
  await Promise.all(writes)
  if (frames.length < 2) throw new Error(`screencast for ${name} produced ${frames.length} frames`)
  await writeFile(path.join(dir, "frames.json"), JSON.stringify({ started, stopped, frames }, null, 2))
  return dir
}

async function signInToDemo() {
  const signedIn = await app.evaluate(`!document.querySelector("#username")`)
  if (signedIn) return
  await app.waitFor(`document.querySelector("#server")?.value === ${JSON.stringify(DEMO_URL)}`)
  await app.type("#username", "demo")
  await app.click("button[type=submit]")
  await app.waitFor(`!!document.querySelector("a[href='/settings']")`, { timeout: 30000 })
}

async function playMuted() {
  await app.evaluate(`[...document.querySelectorAll("button")].find((button) => ["Play", "Resume"].includes(button.textContent.trim())).click(), true`)
  await app.waitFor(`(() => {
    const mute = document.querySelector('button[aria-label="Mute"]')
    if (!mute) return false
    mute.click()
    return true
  })()`, { timeout: 30000, interval: 20 })
  await app.waitFor(`!!document.querySelector('button[aria-label="Unmute"]')`)
}

async function stopPlayback() {
  await app.evaluate(`document.querySelector('button[aria-label="Stop"]')?.click(), true`)
  await sleep(1500)
}

// The seek thumb's aria-valuenow is the player position in milliseconds.
const POSITION = `+(document.querySelector(".player-seek-target [role=slider]")?.getAttribute("aria-valuenow") ?? NaN)`

// Page frames with a transparent background while the film plays. Each frame
// records the film position it shows: the position is anchored at the moment
// the app reports a new value, and advances with the wall clock from there.
async function overlayFrames(name, { from, seconds, interval = 80 }) {
  await app.waitFor(`${POSITION} >= ${from}`, { timeout: 90000, interval: 100 })
  const dir = path.join(rawDir, name)
  await rm(dir, { recursive: true, force: true })
  await mkdir(dir, { recursive: true })
  await app.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } })
  let stopWiggle = false
  const wiggle = (async () => {
    for (let step = 0; !stopWiggle; step++) {
      const height = await app.evaluate("innerHeight")
      // Inside the reveal band but above the bar, so no seek tooltip shows.
      await app.mouse("mouseMoved", 1000 + (step % 2) * 40, height - 90)
      await sleep(350)
    }
  })()
  try {
    await sleep(800)
    const previous = await app.evaluate(POSITION)
    let anchor = previous
    while (anchor === previous) anchor = await app.evaluate(POSITION)
    const anchoredAt = performance.now()
    const frames = []
    while (performance.now() - anchoredAt < seconds * 1000) {
      const before = performance.now()
      const { data } = await app.send("Page.captureScreenshot", { format: "png" })
      const at = (before + performance.now()) / 2
      const file = `${String(frames.length).padStart(4, "0")}.png`
      await writeFile(path.join(dir, file), Buffer.from(data, "base64"))
      frames.push({ file, positionMs: Math.round(anchor + at - anchoredAt) })
      await sleep(Math.max(0, interval - (performance.now() - before)))
    }
    await writeFile(path.join(dir, "frames.json"), JSON.stringify({ frames }, null, 2))
    return dir
  } finally {
    stopWiggle = true
    await wiggle
    await app.send("Emulation.setDefaultBackgroundColorOverride", {})
  }
}

// The demo server's original file for an item, fetched with the demo
// account's own token so the composite uses exactly what the app played.
async function downloadDemoSource(itemId) {
  const file = path.join(rawDir, `source-${itemId}.mp4`)
  if (existsSync(file)) return file
  const auth = 'MediaBrowser Client="MediaFlick website capture", Device="capture", DeviceId="mediaflick-website-capture", Version="1"'
  const login = await fetch(`${DEMO_URL}/Users/AuthenticateByName`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: auth },
    body: JSON.stringify({ Username: "demo", Pw: "" }),
  })
  if (!login.ok) throw new Error(`demo sign-in failed: ${login.status}`)
  const { AccessToken: token } = await login.json()
  const response = await fetch(`${DEMO_URL}/Videos/${itemId}/stream?static=true`, {
    headers: { Authorization: `${auth}, Token="${token}"` },
  })
  if (!response.ok) throw new Error(`demo download failed: ${response.status}`)
  await writeFile(file, Buffer.from(await response.arrayBuffer()))
  return file
}

// The sign-in screen is captured by the runner before it signs in, as scene
// `signin`; the rest need a session.
const demoScenes = {
  async home() {
    await go("/")
    await park()
    await settled(1500)
    return { still: await still("home") }
  },
  async "home-hover"() {
    await go("/")
    await park()
    const card = `article.signal-card a[aria-label="Open details for Caminandes: Llamigos"]`
    await app.waitFor(`!!document.querySelector(${JSON.stringify(card)})`)
    await app.evaluate(`document.querySelector(${JSON.stringify(card)}).scrollIntoView({ block: "center" }), true`)
    await settled(900)
    const recording = await record("home-hover", async () => {
      await sleep(500)
      await app.hover(card)
      await sleep(2600)
    })
    return { still: await still("home-hover"), recording }
  },
  async movies() {
    await go("/library?kind=Movie")
    await park()
    await settled(1200)
    return { still: await still("movies") }
  },
  async "movie-detail"() {
    await go("/")
    const href = await linkFor("Night of the Living Dead")
    await go(href)
    await park()
    await settled(1500)
    return { still: await still("movie-detail") }
  },
  async "series-detail"() {
    await go("/library?kind=Series")
    const href = await linkFor("Pioneer One")
    await go(href)
    await park()
    await settled(1500)
    return { still: await still("series-detail") }
  },
  // The sidebar is open on Home or under the pointer, and a search moves to
  // the library, so the pointer rests on the sidebar while typing and then
  // leaves it, as a user's would.
  async search() {
    await go("/")
    await park()
    const input = `input[aria-label="Search the library"]`
    const recording = await record("search", async () => {
      await app.mouse("mouseMoved", 140, 420)
      await sleep(500)
      await app.type(input, "the great", { delay: 140 })
      await sleep(1400)
      await park()
      await sleep(1600)
    })
    const result = { still: await still("search"), recording }
    await app.route("/")
    return result
  },
  // libmpv draws the video natively beneath a transparent page, so neither
  // CDP nor a desktop grab of a background window sees both. This scene plays
  // the film for real, captures the page with alpha at the film's exact
  // position, and downloads the same film so encode.mjs can composite the
  // two. Audio is muted the moment the player appears.
  async player() {
    await viewport({ width: 1600, height: 900 })
    await go("/library?kind=Movie")
    const href = await linkFor("Caminandes: Llama Drama")
    await go(href)
    const source = await downloadDemoSource(href.slice("/item/".length))
    await playMuted()
    const overlay = await overlayFrames("player-overlay", { from: 14000, seconds: 6 })
    await stopPlayback()
    return { source, overlay }
  },
  // The site's hero loop: Home, a scroll to the shelves, a hover card and the
  // details page as a CDP screencast, ending on the Play click. encode.mjs
  // continues it with the composited player clip.
  async tour() {
    await go("/")
    await park()
    await settled(1500)
    const card = `article.signal-card a[aria-label="Open details for Caminandes: Llama Drama"]`
    await app.waitFor(`!!document.querySelector(${JSON.stringify(card)})`)
    const recording = await record("tour", async () => {
      await sleep(2600)
      await app.evaluate(`document.querySelector(${JSON.stringify(card)}).scrollIntoView({ block: "center", behavior: "smooth" }), true`)
      await sleep(1400)
      await app.hover(card)
      await sleep(2400)
      await app.click(card)
      await app.waitFor(`location.pathname.startsWith("/item/")`, { timeout: 5000 })
      await sleep(3000)
      await app.evaluate(`[...document.querySelectorAll("button")].find((button) => ["Play", "Resume"].includes(button.textContent.trim())).dataset.capturePlay = "", true`)
      await app.hover("[data-capture-play]")
      await sleep(700)
    }, { tail: 0 })
    return { recording }
  },
  async appearance() {
    await go("/settings/appearance")
    await park()
    return { still: await still("appearance") }
  },
}

// The home profile belongs to a real person on a real server. Their account
// name and server host are swapped for neutral stand-ins in every text node,
// including ones rendered later, before anything is captured.
async function anonymize() {
  const identity = await app.evaluate(`(() => {
    const footer = document.querySelector("[data-sidebar=footer]")
    const lines = (footer?.innerText ?? "").split("\\n").map((line) => line.trim()).filter(Boolean)
    const host = lines.find((line) => /^[a-z0-9-]+(\\.[a-z0-9-]+)+(:\\d+)?$/i.test(line))
    const name = host ? lines[lines.indexOf(host) - 1] : undefined
    return { host, name }
  })()`)
  if (!identity.host || !identity.name) throw new Error("could not find the account footer to anonymize")
  const replacements = [
    [identity.host, "jellyfin.local"],
    [identity.name, "cinema"],
    // The avatar initial; CSS may uppercase it.
    [identity.name[0], "c"],
  ]
  await app.evaluate(`(() => {
    // Every capture uses the default Signal accent so the site's images match
    // each other; the profile's own accent choice would otherwise leak in.
    const root = document.documentElement
    const keepSignal = () => { if (root.dataset.accent) delete root.dataset.accent }
    keepSignal()
    new MutationObserver(keepSignal).observe(root, { attributes: true, attributeFilter: ["data-accent"] })
    // The copied profile keeps syncing in the background; its progress card is
    // transient status, not part of any page being shown.
    const style = document.createElement("style")
    style.textContent = ".sidebar-sync-progress { display: none !important }"
    document.head.append(style)
    const replacements = ${JSON.stringify(replacements)}
    const rewrite = (node) => {
      const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT)
      for (let text = walker.nextNode(); text; text = walker.nextNode()) {
        const value = text.nodeValue.trim()
        for (const [from, to] of replacements) {
          // The avatar initial is replaced only when it is the whole text node.
          if (from.length === 1 ? value === from : text.nodeValue.includes(from)) {
            text.nodeValue = from.length === 1 ? text.nodeValue.replace(from, to) : text.nodeValue.replaceAll(from, to)
          }
        }
      }
    }
    // Letterboxd reviews come from the account's own friends: real people.
    const hideReviews = () => {
      for (const heading of document.querySelectorAll("h2, h3")) {
        if (heading.textContent.trim() !== "Letterboxd") continue
        const section = heading.closest("section") ?? heading.parentElement
        if (section && section.style.display !== "none") section.style.display = "none"
      }
    }
    rewrite(document.body)
    hideReviews()
    new MutationObserver((records) => {
      hideReviews()
      for (const record of records) {
        if (record.type === "characterData") rewrite(record.target.parentNode ?? document.body)
        for (const added of record.addedNodes) rewrite(added.nodeType === Node.TEXT_NODE ? added.parentNode ?? document.body : added)
      }
    }).observe(document.body, { subtree: true, childList: true, characterData: true })
    return true
  })()`)
  return { replaced: replacements.map(([, to]) => to) }
}

const homeScenes = {
  async "home-full"() {
    await go("/")
    await park()
    await settled(1500)
    return { still: await still("home-full") }
  },
  async timeline() {
    await go("/")
    await park()
    const heading = await app.evaluate(`(() => {
      const heading = [...document.querySelectorAll("h2, h3")].find((element) => element.textContent.trim() === "Release Timeline")
      if (!heading) return false
      heading.scrollIntoView({ block: "start" })
      document.querySelector("main")?.scrollBy?.(0, -24)
      return true
    })()`)
    if (!heading) throw new Error("Release Timeline shelf is not on Home")
    await settled(1500)
    return { still: await still("timeline") }
  },
  // Companion ratings (IMDb, Rotten Tomatoes, Letterboxd and friends) on the
  // details page of the newest film.
  async "rated-detail"() {
    await go("/")
    const href = await app.waitFor(`(() => {
      const heading = [...document.querySelectorAll("h2, h3")].find((element) => element.textContent.trim() === "Recently Added Movies")
      const section = heading?.closest("section") ?? heading?.parentElement?.parentElement
      return section?.querySelector("article.signal-card a[href^='/item/']")?.getAttribute("href")
    })()`)
    await go(href)
    await park()
    await settled(2000)
    return { still: await still("rated-detail") }
  },
  async calendar() {
    await go("/calendar")
    await park()
    await settled(1500)
    return { still: await still("calendar") }
  },
  async discover() {
    await go("/discover")
    await park()
    await settled(1500)
    return { still: await still("discover") }
  },
  async requests() {
    await go("/requests")
    await park()
    await settled(1500)
    return { still: await still("requests") }
  },
  async collections() {
    await go("/collections/franchises")
    await park()
    await settled(1500)
    return { still: await still("collections") }
  },
}

const scenes = profile === "demo" ? demoScenes : homeScenes
const report = { profile, viewport: VIEWPORT, capturedAt: new Date().toISOString(), scenes: {} }
const reportFile = path.join(rawDir, `report-${profile}.json`)
if (existsSync(reportFile) && only.size) Object.assign(report.scenes, JSON.parse(await readFile(reportFile, "utf8")).scenes)

let failed = false
try {
  await viewport()
  if (profile === "demo") {
    if (!only.size || only.has("signin")) {
      await app.waitFor(`!!(document.querySelector("#username") || document.querySelector("a[href='/settings']"))`, { timeout: 30000 })
      if (await app.evaluate(`!!document.querySelector("#username")`)) {
        await sleep(800)
        report.scenes.signin = { ok: true, still: await still("signin") }
      }
    }
    await signInToDemo()
  } else {
    await app.waitFor(`!!document.querySelector("a[href='/settings']")`, { timeout: 30000 })
    // The copied profile resumes its background library sync first.
    await app.waitFor(`!/Loading library/.test(document.querySelector("[data-sidebar=footer]")?.innerText ?? "")`, { timeout: 300000, interval: 1000 })
    report.anonymized = await anonymize()
  }
  for (const [name, scene] of Object.entries(scenes)) {
    if (only.size && !only.has(name)) continue
    console.log(`scene ${name}`)
    try {
      await viewport()
      const outputs = await scene()
      report.scenes[name] = { ok: true, route: await app.evaluate("location.pathname + location.search"), ...outputs }
    } catch (error) {
      failed = true
      report.scenes[name] = { ok: false, error: String(error?.message ?? error) }
      console.error(`  ${name} failed: ${error?.message ?? error}`)
    }
  }
} finally {
  await writeFile(reportFile, JSON.stringify(report, null, 2))
  app.close()
  await stop(profile)
}
console.log(`report: ${path.relative(root, reportFile)}`)
if (failed) process.exitCode = 1
