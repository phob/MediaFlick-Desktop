// End-to-end check of the Home "Release Timeline" shelf (built-in id `upcoming`).
//
// Drives a running, signed-in MediaFlick Desktop through its real path:
// Desktop -> Jellyfin library and Companion calendar (Sonarr, Radarr). Start
// the app with `--remote-debugging-port 9334` (or set CDP_PORT) with the
// Release Timeline shelf enabled on Home, then run:
//
//   node scripts/e2e/upcoming-shelf.mjs [output-dir] [scenario]
//
// Scenario `live` (default) checks the server's real calendar. Scenario
// `missing` covers a release that has not arrived even when the server has
// none: it rewrites the app's calendar response in flight so the latest
// released movie has no file and no library match, and expects its card to
// say Missing. Everything else still runs through the real app.
//
// The output directory (default build/e2e/upcoming-shelf[-missing]) receives
// report.json and screenshots of the shelf at its start and at the first
// upcoming card. The script exits non-zero when an invariant fails.

import { mkdir, writeFile } from "node:fs/promises"
import path from "node:path"

const port = Number(process.env.CDP_PORT ?? 9334)
const scenario = process.argv[3] ?? "live"
if (!["live", "missing"].includes(scenario)) throw new Error(`unknown scenario ${scenario}`)
const outDir = process.argv[2] ?? `build/e2e/upcoming-shelf${scenario === "live" ? "" : `-${scenario}`}`
const SHELF_TITLE = "Release Timeline"
const SHELF_LIMIT = 24
const RECENT_SHARE = 0.3
const RECENT_DAYS = 30
const UPCOMING_DAYS = 90

const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
const page = targets.find((target) => target.type === "page" && target.url.startsWith("mediaflick-desktop://app"))
if (!page) throw new Error(`no MediaFlick page on CDP port ${port}`)

const socket = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((resolve, reject) => {
  socket.onopen = resolve
  socket.onerror = reject
})
let nextId = 0
const pending = new Map()
socket.onmessage = (event) => {
  const message = JSON.parse(event.data)
  const waiter = pending.get(message.id)
  if (!waiter) return
  pending.delete(message.id)
  if (message.error) waiter.reject(new Error(message.error.message))
  else waiter.resolve(message.result)
}
function send(method, params = {}) {
  const id = ++nextId
  socket.send(JSON.stringify({ id, method, params }))
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }))
}
async function evaluate(expression) {
  const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? "evaluation failed")
  return result.result.value
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

function isoDate(date) {
  const month = String(date.getMonth() + 1).padStart(2, "0")
  const day = String(date.getDate()).padStart(2, "0")
  return `${date.getFullYear()}-${month}-${day}`
}
const now = new Date()
const today = isoDate(now)
const windowStart = isoDate(new Date(now.getFullYear(), now.getMonth(), now.getDate() - RECENT_DAYS))
const windowEnd = isoDate(new Date(now.getFullYear(), now.getMonth(), now.getDate() + UPCOMING_DAYS))

const failures = []
const check = (condition, message) => {
  if (!condition) failures.push(message)
}

// The `missing` scenario: the latest movie released in the window loses its
// file and library match in every calendar response the app receives. CDP
// request interception does not see the app's custom scheme, so the page's
// own fetch is wrapped before the app loads.
if (scenario === "missing") {
  await send("Page.addScriptToEvaluateOnNewDocument", {
    source: `(() => {
      const fetch = window.fetch.bind(window)
      window.fetch = async (input, init) => {
        const response = await fetch(input, init)
        const url = typeof input === "string" ? input : input.url
        if (!url.includes("/api/calendar") || !response.ok) return response
        const calendar = await response.json()
        const released = (calendar.entries ?? [])
          .filter((entry) => entry.kind === "movie" && entry.date < ${JSON.stringify(today)} && entry.dateKind !== "cinema")
          .sort((left, right) => right.date.localeCompare(left.date))[0]
        if (released) {
          window.__e2eSimulatedMissing ??= { title: released.title, tmdbId: released.tmdbId }
          for (const entry of calendar.entries) {
            if (entry.kind === "movie" && entry.tmdbId === window.__e2eSimulatedMissing.tmdbId) {
              entry.hasFile = false
              entry.libraryItemId = null
            }
          }
        }
        return new Response(JSON.stringify(calendar), { status: response.status, headers: response.headers })
      }
    })()`,
  })
}

// Reload for fresh data, then route to Home in-app: a reload itself follows
// the startup-page setting, which may not be Home.
await send("Page.enable")
await send("Page.reload", { ignoreCache: true })
await sleep(3000)
let shelfReady = false
for (let attempt = 0; attempt < 60 && !shelfReady; attempt++) {
  if (attempt % 10 === 0) {
    await evaluate(`(location.pathname === "/" || (history.pushState(null, "", "/"), dispatchEvent(new PopStateEvent("popstate"))), true)`).catch(() => false)
  }
  await sleep(500)
  shelfReady = await evaluate(`(() => {
    const heading = [...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(SHELF_TITLE)})
    return Boolean(heading?.closest("section")?.querySelector("article"))
  })()`).catch(() => false)
}
if (!shelfReady) throw new Error("the Home release timeline did not render within 30 seconds")

// The same calendar Desktop's Home reads, fetched through the app's API.
const simulatedMissing = scenario === "missing" ? await evaluate("window.__e2eSimulatedMissing ?? null") : null
const calendar = await evaluate(
  `fetch("/api/calendar?start=${windowStart}&end=${windowEnd}").then((response) => response.json())`,
)

// Let artwork settle before measuring it.
await sleep(2500)
const shelf = await evaluate(`(() => {
  const heading = [...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(SHELF_TITLE)})
  const section = heading.closest("section")
  const posterFrame = [...document.querySelectorAll("section")]
    .filter((candidate) => candidate !== section)
    .flatMap((candidate) => [...candidate.querySelectorAll("article .media-frame")])
    .map((frame) => frame.getBoundingClientRect())
    .find((rect) => rect.height > rect.width)
  const cards = [...section.querySelectorAll("article")].map((article) => {
    const frame = article.querySelector(".media-frame")
    const rect = frame.getBoundingClientRect()
    const link = frame.querySelector("a")
    const image = frame.querySelector("img")
    const lines = [...article.querySelectorAll(":scope > a div")].map((node) => node.textContent)
    const labels = [...frame.querySelectorAll("span[title]")].map((node) => node.title)
    const date = [...frame.querySelectorAll(".data-label")].map((node) => node.textContent).find((text) => text !== "NEW SEASON")
    return {
      ariaLabel: link?.getAttribute("aria-label") ?? null,
      href: link?.getAttribute("href") ?? null,
      title: lines[0] ?? null,
      subtitle: lines[1] ?? null,
      dateLabel: date ?? null,
      statusLabels: labels,
      newSeason: frame.textContent.includes("NEW SEASON"),
      frame: { width: Math.round(rect.width), height: Math.round(rect.height) },
      image: image ? { src: image.getAttribute("src"), loaded: image.complete && image.naturalWidth > 0 } : null,
    }
  })
  const articles = [...section.querySelectorAll("article")]
  const todayMarks = [...section.querySelectorAll('[role="separator"][aria-label="Today"]')].map((mark) => {
    const rect = mark.getBoundingClientRect()
    const previous = mark.previousElementSibling?.getBoundingClientRect()
    const next = mark.nextElementSibling?.getBoundingClientRect()
    return {
      nextCardIndex: articles.indexOf(mark.nextElementSibling),
      offsetFromGapCenter: previous && next ? Math.round((rect.left + rect.width / 2 - (previous.right + next.left) / 2) * 10) / 10 : null,
      gapAcross: previous && next ? Math.round(next.left - previous.right) : null,
      height: Math.round(rect.height),
    }
  })
  const regularGaps = articles.slice(1).map((article, index) => Math.round(article.getBoundingClientRect().left - articles[index].getBoundingClientRect().right))
  const dateLabel = (date) => new Date(date + "T12:00:00").toLocaleDateString(undefined, { month: "short", day: "numeric" })
  return {
    cards,
    todayMarks,
    regularGap: regularGaps.length ? Math.min(...regularGaps) : null,
    posterReference: posterFrame ? { width: Math.round(posterFrame.width), height: Math.round(posterFrame.height) } : null,
    dateLabels: Object.fromEntries(${JSON.stringify(calendar.entries?.map((entry) => entry.date) ?? [])}.map((date) => [date, dateLabel(date)])),
  }
})()`)

const entries = calendar.entries ?? []
const isDownloaded = (entry) => entry.hasFile || entry.libraryItemId != null
const entryTitle = (entry) => (entry.kind === "episode" ? entry.seriesTitle ?? entry.title : entry.title)

const cards = shelf.cards.map((card, index) => {
  // Every calendar entry this card could stand for: same displayed title and date.
  const matches = entries.filter((entry) => entryTitle(entry) === card.title && shelf.dateLabels[entry.date] === card.dateLabel)
  const dates = [...new Set(matches.map((entry) => entry.date))]
  const date = dates.length === 1 ? dates[0] : null
  const past = date != null && date < today
  const status = card.statusLabels.length === 1 ? card.statusLabels[0] : null
  const posterSource = card.image?.src?.startsWith("/api/image/")
    ? "jellyfin"
    : card.image?.src?.startsWith("/api/collections/provider-artwork")
      ? "tmdb"
      : "placeholder"
  return { index, ...card, date, past, status, posterSource, matches: matches.length, anyDownloaded: matches.some(isDownloaded), allDownloaded: matches.length > 0 && matches.every(isDownloaded) }
})

check(calendar.provider === "plugin", `calendar provider is ${calendar.provider}; past-release truth needs the Companion`)
check(cards.length > 0 && cards.length <= SHELF_LIMIT, `shelf holds ${cards.length} cards; expected 1..${SHELF_LIMIT}`)

// 1. Poster format, identical to the other poster shelves on Home.
check(shelf.posterReference != null, "no other poster shelf on Home to compare the card size with")
for (const card of cards) {
  check(card.frame.height > card.frame.width, `card ${card.index} (${card.title}) is not portrait: ${card.frame.width}x${card.frame.height}`)
  if (shelf.posterReference) {
    check(
      card.frame.width === shelf.posterReference.width && card.frame.height === shelf.posterReference.height,
      `card ${card.index} (${card.title}) is ${card.frame.width}x${card.frame.height}, other posters are ${shelf.posterReference.width}x${shelf.posterReference.height}`,
    )
  }
}

// Every card must be traceable to the calendar it came from.
for (const card of cards) check(card.date != null, `card ${card.index} (${card.title} @ ${card.dateLabel}) matches no single calendar date`)

// 2. One timeline: dates never go backwards, and every past card precedes every upcoming card.
for (let index = 1; index < cards.length; index++) {
  const previous = cards[index - 1].date
  const current = cards[index].date
  if (previous && current) check(previous <= current, `card ${index} (${current}) comes after a later date (${previous})`)
}

// 3. The recent share: 30% of the shelf when both sides can fill theirs.
const pastCards = cards.filter((card) => card.past).length
const futureCards = cards.length - pastCards
const eligiblePast = new Set(
  entries
    .filter((entry) => entry.date < today && !(entry.kind === "movie" && entry.dateKind === "cinema"))
    .map((entry) => `${entry.date}:${entryTitle(entry)}`),
).size
const eligibleFuture = new Set(entries.filter((entry) => entry.date >= today).map((entry) => `${entry.date}:${entryTitle(entry)}`)).size
const recentTarget = Math.round(SHELF_LIMIT * RECENT_SHARE)
if (eligiblePast >= recentTarget && eligibleFuture >= SHELF_LIMIT - recentTarget) {
  check(pastCards === recentTarget, `shelf shows ${pastCards} past cards; expected ${recentTarget} (30% of ${SHELF_LIMIT})`)
  check(futureCards === SHELF_LIMIT - recentTarget, `shelf shows ${futureCards} upcoming cards; expected ${SHELF_LIMIT - recentTarget}`)
} else {
  check(cards.length === Math.min(SHELF_LIMIT, pastCards + futureCards), "shelf left slots empty that the other side could fill")
}
check(eligiblePast === 0 || pastCards > 0, `the calendar has ${eligiblePast} recent releases but the shelf shows none`)

// 4 and 5. Past cards carry exactly one truthful status; upcoming cards carry none.
for (const card of cards) {
  if (!card.date) continue
  if (!card.past) {
    check(card.statusLabels.length === 0, `upcoming card ${card.index} (${card.title}) shows a status: ${card.statusLabels.join(", ")}`)
    continue
  }
  check(card.statusLabels.length === 1, `past card ${card.index} (${card.title}) shows ${card.statusLabels.length} status indicators`)
  if (card.status === "Missing") check(!card.anyDownloaded, `past card ${card.index} (${card.title}) says Missing but a matching release is downloaded`)
  if (card.status === "Downloaded") check(card.allDownloaded, `past card ${card.index} (${card.title}) says Downloaded but a matching release has no file`)
  check(card.ariaLabel?.endsWith(`, ${card.status}`), `past card ${card.index} (${card.title}) does not announce its status: ${card.ariaLabel}`)
}

// 6. Today is marked once, centred in the gap between the last past and the
// first upcoming card, without widening that gap.
const firstFuture = cards.findIndex((card) => card.date && !card.past)
if (pastCards > 0 && firstFuture > 0) {
  check(shelf.todayMarks.length === 1, `expected one today mark, found ${shelf.todayMarks.length}`)
  const [mark] = shelf.todayMarks
  if (mark) {
    check(mark.nextCardIndex === firstFuture, `today mark precedes card ${mark.nextCardIndex}; the first upcoming card is ${firstFuture}`)
    check(mark.offsetFromGapCenter != null && Math.abs(mark.offsetFromGapCenter) <= 1, `today mark is ${mark.offsetFromGapCenter}px off the gap centre`)
    check(mark.gapAcross === shelf.regularGap, `the gap across the today mark is ${mark.gapAcross}px; cards are otherwise ${shelf.regularGap}px apart`)
    check(mark.height === shelf.posterReference?.height, `today mark is ${mark.height}px tall; posters are ${shelf.posterReference?.height}px`)
  }
} else {
  check(shelf.todayMarks.length === 0, "the shelf marks today although it has no boundary between past and upcoming")
}

if (scenario === "missing") {
  check(simulatedMissing != null, "the calendar has no released movie in the window to mark as missing")
  const card = simulatedMissing && cards.find((candidate) => candidate.title === simulatedMissing.title && candidate.past)
  check(card != null, `the missing movie ${simulatedMissing?.title} has no past card on the shelf`)
  if (card) check(card.status === "Missing", `the missing movie ${card.title} shows ${card.status ?? "no status"}, not Missing`)
}

// Screenshots: the shelf's start (recent releases), then scrolled to the first upcoming card.
await mkdir(outDir, { recursive: true })
async function shelfScreenshot(file) {
  const clip = await evaluate(`(() => {
    const heading = [...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(SHELF_TITLE)})
    const section = heading.closest("section")
    section.scrollIntoView({ block: "center" })
    const rect = section.getBoundingClientRect()
    return { x: 0, y: Math.max(0, rect.top), width: document.documentElement.clientWidth, height: rect.height, scale: 1 }
  })()`)
  await sleep(600)
  const shot = await send("Page.captureScreenshot", { format: "png", clip })
  await writeFile(path.join(outDir, file), Buffer.from(shot.data, "base64"))
}
await shelfScreenshot("upcoming-start.png")
const firstUpcoming = cards.find((card) => card.date && !card.past)
if (firstUpcoming && firstUpcoming.index > 0) {
  await evaluate(`(() => {
    const heading = [...document.querySelectorAll("h2")].find((node) => node.textContent === ${JSON.stringify(SHELF_TITLE)})
    const rail = heading.closest("section").querySelector("[role=region]")
    const card = rail.querySelectorAll("article")[${firstUpcoming.index - 1}]
    rail.scrollLeft = card.offsetLeft - rail.offsetLeft - 40
    return true
  })()`)
  await shelfScreenshot("upcoming-today-boundary.png")
}

const report = {
  generatedAt: new Date().toISOString(),
  scenario,
  simulatedMissing,
  today,
  window: { start: windowStart, end: windowEnd },
  calendarProvider: calendar.provider,
  calendarEntries: entries.length,
  eligible: { past: eligiblePast, future: eligibleFuture },
  shelf: {
    todayMarks: shelf.todayMarks,
    regularGap: shelf.regularGap,
    cards: cards.length,
    past: pastCards,
    upcoming: futureCards,
    downloaded: cards.filter((card) => card.status === "Downloaded").length,
    missing: cards.filter((card) => card.status === "Missing").length,
    posters: {
      reference: shelf.posterReference,
      jellyfin: cards.filter((card) => card.posterSource === "jellyfin" && card.image?.loaded).length,
      tmdb: cards.filter((card) => card.posterSource === "tmdb" && card.image?.loaded).length,
      placeholder: cards.filter((card) => !card.image?.loaded).length,
    },
  },
  cards: cards.map(({ index, title, subtitle, date, past, status, newSeason, ariaLabel, href, frame, posterSource, image, matches }) => ({
    index, title, subtitle, date, past, status, newSeason, ariaLabel, href, frame, posterSource, imageLoaded: image?.loaded ?? false, calendarMatches: matches,
  })),
  failures,
  passed: failures.length === 0,
}
await writeFile(path.join(outDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`)
socket.close()

console.log(`${report.shelf.cards} cards: ${pastCards} past (${report.shelf.downloaded} downloaded, ${report.shelf.missing} missing), ${futureCards} upcoming`)
console.log(`posters: ${report.shelf.posters.jellyfin} Jellyfin, ${report.shelf.posters.tmdb} TMDB, ${report.shelf.posters.placeholder} placeholder`)
console.log(`report: ${path.join(outDir, "report.json")}`)
if (failures.length) {
  for (const failure of failures) console.error(`FAIL ${failure}`)
  process.exit(1)
}
console.log("PASS")
