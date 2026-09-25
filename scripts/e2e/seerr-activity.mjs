// End-to-end check of Seerr request activity badges.
//
// Drives a running, signed-in MediaFlick Desktop through its real path:
// Desktop -> Companion -> Seerr, Radarr, and Sonarr. Start the app with
// `--remote-debugging-port 9333` against a server whose Companion is
// configured with Seerr, then run:
//
//   node scripts/e2e/seerr-activity.mjs [output-dir]
//
// The output directory (default build/e2e/seerr-activity) receives
// report.json and screenshots of the Requests page and each in-progress title's
// detail page. The script exits non-zero when an invariant fails.

import { mkdir, writeFile } from "node:fs/promises"
import path from "node:path"

const port = Number(process.env.CDP_PORT ?? 9333)
const outDir = process.argv[2] ?? "build/e2e/seerr-activity"
const ACTIVITIES = ["downloading", "searching", "in-cinemas", "unreleased", "awaiting-episodes"]
const LABELS = {
  downloading: "Downloading",
  searching: "Searching",
  "in-cinemas": "In cinemas",
  unreleased: "Unreleased",
  "awaiting-episodes": "Awaiting episodes",
}

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
const api = (url) => evaluate(`fetch(${JSON.stringify(url)}).then((response) => response.json())`)
const IN_PROGRESS_LABELS = [...Object.values(LABELS), "Processing"]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function visit(route, file, readyText) {
  await evaluate(`history.pushState({}, "", ${JSON.stringify(route)}); dispatchEvent(new PopStateEvent("popstate"))`)
  for (let attempt = 0; attempt < 40; attempt++) {
    await sleep(250)
    if (!readyText || (await evaluate(`document.body.innerText.includes(${JSON.stringify(readyText)})`))) break
  }
  await sleep(1500)
  const shot = await send("Page.captureScreenshot", { format: "png" })
  await writeFile(path.join(outDir, file), Buffer.from(shot.data, "base64"))
  const badges = await evaluate(
    `[...document.querySelectorAll('[data-slot="badge"]')].map((badge) => ({ label: badge.textContent.trim(), description: badge.title }))`,
  )
  // Every in-progress badge explains itself on hover, including the
  // "Processing" fallback shown when Radarr or Sonarr cannot say more.
  for (const badge of badges.filter((badge) => IN_PROGRESS_LABELS.includes(badge.label))) {
    check(badge.description.length > 0, `${route}: the "${badge.label}" badge has no description`)
  }
  return badges.map((badge) => badge.label)
}

await mkdir(outDir, { recursive: true })
const failures = []
const check = (condition, message) => {
  if (!condition) failures.push(message)
}

const requests = (await api("/api/seerr/requests?filter=all&take=100")).results
const inProgress = requests.filter((request) => request.mediaStatus === "processing")
const titles = []
for (const request of inProgress) {
  const detail = await api(`/api/seerr/media/${request.mediaType}/${request.tmdbId}`)
  const title = {
    mediaType: request.mediaType,
    tmdbId: request.tmdbId,
    title: detail.title,
    is4k: request.is4k,
    requestMediaStatus: request.mediaStatus,
    requestMediaActivity: request.mediaActivity,
    detailStatus: detail.status,
    detailActivity: detail.activity,
    inLibrary: Boolean(detail.libraryItemId),
    seasons: detail.seasons
      .filter((season) => season.status === "processing")
      .map(({ seasonNumber, status, activity }) => ({ seasonNumber, status, activity })),
  }
  titles.push(title)

  const label = `${title.title} (${title.mediaType} ${title.tmdbId})`
  check("mediaActivity" in request, `${label}: the request carries no mediaActivity; is the Companion current?`)
  check(
    request.mediaActivity === null || ACTIVITIES.includes(request.mediaActivity),
    `${label}: unknown activity ${request.mediaActivity}`,
  )
  if (!request.is4k && detail.status === "processing") {
    check(
      request.mediaActivity === detail.activity,
      `${label}: Requests says ${request.mediaActivity} but the title page says ${detail.activity}`,
    )
  }
  for (const season of title.seasons) {
    check(
      season.activity === null || ACTIVITIES.includes(season.activity),
      `${label} season ${season.seasonNumber}: unknown activity ${season.activity}`,
    )
  }
}

for (const request of requests.filter((request) => request.mediaStatus !== "processing")) {
  check(
    request.mediaActivity == null,
    `request ${request.id}: ${request.mediaStatus} must not carry activity ${request.mediaActivity}`,
  )
}

const discover = (await api("/api/seerr/discover/upcoming-movies?page=1")).results
for (const result of discover) {
  check(
    result.status === "processing" || result.activity == null,
    `${result.title}: ${result.status} must not carry activity ${result.activity}`,
  )
  const request = titles.find((title) => title.mediaType === result.mediaType && title.tmdbId === result.tmdbId && !title.is4k)
  if (request && result.status === "processing") {
    check(
      result.activity === request.detailActivity,
      `${result.title}: Discovery says ${result.activity} but the title page says ${request.detailActivity}`,
    )
  }
}

// The Requests page must render exactly one badge per in-progress, not-in-library
// request, labelled by its activity, and never the old unconditional "Downloading".
const requestBadges = await visit("/requests", "requests.png", "Requests")
const expectedLabels = requests
  .filter((request) => request.mediaStatus === "processing" && !request.libraryItemId)
  .map((request) => LABELS[request.mediaActivity] ?? "Processing")
for (const label of new Set(expectedLabels)) {
  const expected = expectedLabels.filter((value) => value === label).length
  const rendered = requestBadges.filter((value) => value === label).length
  check(rendered >= expected, `Requests page shows ${rendered} "${label}" badges, expected at least ${expected}`)
}
check(
  requestBadges.filter((value) => value === "Downloading").length === expectedLabels.filter((value) => value === "Downloading").length,
  `Requests page shows "Downloading" for a title that is not downloading`,
)

const detailPages = []
for (const title of titles.filter((title) => !title.is4k)) {
  const file = `detail-${title.mediaType}-${title.tmdbId}.png`
  const badges = await visit(`/discover/${title.mediaType}/${title.tmdbId}`, file, title.title)
  // A title already in the library shows that instead of its Seerr status.
  const expected = title.inLibrary ? "In your library" : (LABELS[title.detailActivity] ?? "Processing")
  check(badges.includes(expected), `${title.title}: detail page does not show "${expected}" (badges: ${badges.join(", ")})`)
  detailPages.push({ title: title.title, screenshot: file, badges })
}

const discoverBadges = await visit("/discover", "discover.png", "Discover")

const report = {
  generatedAt: new Date().toISOString(),
  passed: failures.length === 0,
  failures,
  inProgressTitles: titles,
  upcomingMovies: discover.map(({ title, tmdbId, status, activity }) => ({ title, tmdbId, status, activity })),
  pages: {
    requests: { screenshot: "requests.png", badges: requestBadges },
    discover: { screenshot: "discover.png", badges: discoverBadges },
    details: detailPages,
  },
}
await writeFile(path.join(outDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`)
socket.close()

for (const title of titles) {
  console.log(`${title.detailActivity ?? "(none)"}\t${title.mediaType}\t${title.title}`)
}
console.log(failures.length ? `FAILED:\n- ${failures.join("\n- ")}` : "passed")
console.log(`report: ${path.join(outDir, "report.json")}`)
process.exitCode = failures.length ? 1 : 0
