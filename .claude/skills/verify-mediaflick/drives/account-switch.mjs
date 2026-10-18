// A response that was in flight while the user switched accounts must not be
// answered with the old account's data, expire the new account, or write to
// either account's local state. The app answers it 409 (`the Jellyfin
// account changed while the request was running`) with `accountChanged: true`
// in the body, which tells it apart from the route's own 409s.
// The app signs in to account A through a loopback proxy that holds chosen
// upstream requests on gates. While they are held the user signs out through
// the user menu and signs in to the demo server as `demo` (account B); then
// the gates open. Three cases:
// - reads whose response arrives after the switch (media info, a series'
//   Next Up, Home's resume row, a movie's trailer, and the reads the detail
//   page of another A movie sends by itself);
// - a read the proxy answers 401 after the switch, which must not expire B;
// - a playback-preference save and a favorite toggle whose upstream request
//   lands after the switch, which must answer 409 and leave
//   playback-preferences.json and B's cache alone.
// The proxy answers A's `POST /Sessions/Logout` itself, so A's token stays
// valid for the held requests to reach Jellyfin as they would on a slow
// server; the after-run task signs that token out. The released favorite
// does reach A's server, and the journaled after-run task removes it. Every
// check is recorded in `account-switch.json`; the run fails at the end
// listing each failed one. Signs out of B at the end. Read-only on the demo
// server.
//   just verify account-switch --account https://jellyfin.ph0c.de --signed-out
import { createHash } from "node:crypto"
import { existsSync, readFileSync } from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"
import { readStoredSession, revokeToken } from "../scripts/account.mjs"

const DEMO = "https://demo.jellyfin.org/stable"
const SERVER = { role: "textbox", name: "Server" }
const SETTINGS = { role: "link", name: "Settings" }
const CARD = { css: 'main a[aria-label^="Open details for"]' }
const STALE = "the Jellyfin account changed while the request was running"
const MEDIA_FIELDS = "MediaSources,MediaStreams"
const ABOUT_FIELDS = "Overview,Tags,Studios,People,CriticRating"
const HOLD_ARRIVAL_MS = 15000
const RELEASE_SETTLE_MS = 30000

const status = async (ctx) => (await ctx.api("/api/status")).body
const userMenu = (userName, serverUrl) => ({ role: "button", name: `${userName[0].toUpperCase()} ${userName} ${new URL(serverUrl).host}` })
const excerpt = (text) => (text ?? "").replace(/\s+/g, " ").slice(0, 240)
const markedAccountChanged = (body) => {
  try {
    return JSON.parse(body).accountChanged === true
  } catch {
    return false
  }
}
const sha256 = (file) => (existsSync(file) ? createHash("sha256").update(readFileSync(file)).digest("hex") : null)

const itemsQuery = (url, id, fields) => url.pathname === "/Items" && url.searchParams.get("ids") === id && url.searchParams.get("Fields") === fields
const upstream = {
  media: (id) => ({ label: `GET /Items?ids=${id}&Fields=${MEDIA_FIELDS}`, match: (url, method) => method === "GET" && itemsQuery(url, id, MEDIA_FIELDS) }),
  about: (id) => ({ label: `GET /Items?ids=${id}&Fields=${ABOUT_FIELDS}`, match: (url, method) => method === "GET" && itemsQuery(url, id, ABOUT_FIELDS) }),
  trailer: (id) => ({ label: `GET /Items/${id}/LocalTrailers`, match: (url, method) => method === "GET" && url.pathname === `/Items/${id}/LocalTrailers` }),
  seriesNextUp: (id) => ({ label: `GET /Shows/NextUp?seriesId=${id}`, match: (url, method) => method === "GET" && url.pathname === "/Shows/NextUp" && url.searchParams.get("seriesId") === id }),
  homeNextUp: () => ({ label: "GET /Shows/NextUp (no seriesId)", match: (url, method) => method === "GET" && url.pathname === "/Shows/NextUp" && !url.searchParams.has("seriesId") }),
  favorite: (id) => ({ label: `POST /UserFavoriteItems/${id}`, match: (url, method) => method === "POST" && url.pathname === `/UserFavoriteItems/${id}` }),
}

// A gate holds the first upstream request it matches. A `delay` gate forwards
// it to Jellyfin on release; an `answer` gate replies with `reply` instead.
function gates() {
  const all = []
  const arm = (name, { label, match }, { hook = "delay", reply = null } = {}) => {
    let open
    const released = new Promise((resolve) => { open = resolve })
    const gate = { name, label, hook, match, reply, arrivedAt: null, releasedAt: null, released, release: () => { gate.releasedAt ??= new Date().toISOString(); open() } }
    all.push(gate)
    return gate
  }
  const take = (hook, url, method) => {
    const gate = all.find((candidate) => candidate.hook === hook && !candidate.arrivedAt && candidate.match(url, method))
    if (gate) gate.arrivedAt = new Date().toISOString()
    return gate
  }
  return {
    all,
    arm,
    delay: (url, method) => take("delay", url, method)?.released ?? 0,
    answer: (url, method) => {
      if (method === "POST" && url.pathname === "/Sessions/Logout") return { status: 200, body: {} }
      const gate = take("answer", url, method)
      return gate ? gate.released.then(() => gate.reply) : null
    },
    releaseAll: () => { for (const gate of all) gate.release() },
  }
}

async function issue(ctx, key, method, apiPath, body) {
  const init = body === undefined ? { method } : { method, headers: { "content-type": "application/json" }, body: JSON.stringify(body) }
  await ctx.app.evaluate(`(() => {
    window.__switch ??= {}
    window.__switchSettled ??= {}
    window.__switch[${JSON.stringify(key)}] = fetch(${JSON.stringify(apiPath)}, ${JSON.stringify(init)})
      .then(async (response) => ({ status: response.status, body: await response.text() }), (error) => ({ status: null, body: String(error) }))
      .then((answer) => { window.__switchSettled[${JSON.stringify(key)}] = new Date().toISOString(); return answer })
    return true
  })()`)
  ctx.step(`issued ${method} ${apiPath} from the page as ${key}`)
}

async function waitHeld(ctx, gate) {
  try {
    await ctx.until(() => gate.arrivedAt, `${gate.label} held upstream (${gate.name})`, HOLD_ARRIVAL_MS)
    return true
  } catch {
    ctx.step(`${gate.name}: ${gate.label} never reached the proxy`)
    return false
  }
}

// Keeps the first answer the page's own fetches get for `paths`, so the
// drive can read the body of a request the UI sent by itself.
async function recordPageAnswers(ctx, paths) {
  await ctx.app.evaluate(`(() => {
    const watched = ${JSON.stringify(paths)}
    const original = window.fetch
    window.__pageAnswers = {}
    window.fetch = (input, init) => original(input, init).then((response) => {
      const { pathname } = new URL(typeof input === "string" ? input : input.url, location.href)
      if (watched.includes(pathname)) {
        response.clone().text().then((body) => { window.__pageAnswers[pathname] ??= { status: response.status, body } })
      }
      return response
    })
    return true
  })()`)
}

async function openByUrl(ctx, route) {
  await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(route)}); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(async () => (await ctx.pathname()) === route, `the ${route} route`)
}

function cacheRows(dataDir, id) {
  const database = new DatabaseSync(path.join(dataDir, "library.db"), { readOnly: true })
  try {
    return {
      items: database.prepare("SELECT COUNT(*) AS n FROM items WHERE jellyfin_id = ?").get(id).n,
      userData: database.prepare("SELECT COUNT(*) AS n FROM user_data WHERE jellyfin_id = ?").get(id).n,
    }
  } finally {
    database.close()
  }
}

async function cached(ctx, kind) {
  const body = (await ctx.api(`/api/items?kind=${kind}&limit=200`)).body
  return body?.items ?? []
}

async function hasTitle(ctx, name) {
  for (const kind of ["Movie,Series", "Episode"]) {
    const body = (await ctx.api(`/api/items?${new URLSearchParams({ search: name, kind, limit: "50" })}`)).body
    if ((body?.items ?? []).some((item) => item.name.toLowerCase().includes(name.toLowerCase()))) return true
  }
  return false
}

const errorToasts = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll('[data-sonner-toast][data-type="error"]')].map((toast) => toast.textContent.trim().slice(0, 160))`)

export default async function accountSwitch(ctx) {
  if (!ctx.account?.signedOut) throw new Error("account-switch needs --account SERVER --signed-out")
  const report = { cases: [], checks: [], concurrency: {} }
  const expect = (condition, message, detail) => {
    report.checks.push({ ok: Boolean(condition), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const gate = gates()
  const proxy = await ctx.proxy({ upstream: ctx.account.server, delay: gate.delay, answer: gate.answer })
  try {
    await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
    await ctx.fill(SERVER, proxy.url)
    await ctx.quickConnect({ approve: true, timeout: 60000 })
    ctx.revokeAppSessionAfterRun()
    const a = await status(ctx)
    expect(a.authenticated === true && a.userName === ctx.account.userName, `signed in to A as ${ctx.account.userName} through the proxy`)
    await ctx.until(async () => (await ctx.pathname()) === "/", "A's Home")

    const userId = ctx.account.userId
    const nextUp = (await ctx.jellyfin(`/Shows/NextUp?userId=${userId}&Limit=20`)).Items ?? []
    const { movies, series } = await ctx.until(async () => {
      const found = { movies: await cached(ctx, "Movie"), series: await cached(ctx, "Series") }
      const wanted = nextUp.find((episode) => found.series.some((show) => show.id === episode.SeriesId))
      return found.movies.length >= 2 && (wanted || (found.series.length > 0 && (await status(ctx))?.bootstrap?.state === "ready")) ? found : null
    }, "A's cache to hold two movies and a series", 180000)
    const notFavorite = []
    for (const movie of movies) {
      const { UserData: data } = await ctx.jellyfin(`/Items/${movie.id}?userId=${userId}`)
      if (!data?.IsFavorite) notFavorite.push(movie)
      if (notFavorite.length === 2) break
    }
    if (notFavorite.length < 2) throw new Error("account A needs two movies that are not favorites")
    const [movie, pageMovie] = notFavorite
    const episode = nextUp.find((candidate) => series.some((show) => show.id === candidate.SeriesId))
    const show = episode ? series.find((candidate) => candidate.id === episode.SeriesId) : series[0]
    const homeTitles = ((await ctx.api("/api/home")).body?.rows ?? []).flatMap((row) => row.items.map((item) => item.name))
    const aTitles = [...new Set([movie.name, pageMovie.name, show.name, ...(episode ? [episode.Name] : []), ...homeTitles.slice(0, 12)])].filter((name) => name.length >= 4)
    report.a = { movie: { id: movie.id, name: movie.name }, pageMovie: { id: pageMovie.id, name: pageMovie.name }, series: { id: show.id, name: show.name, nextUp: episode?.Name ?? null } }
    ctx.step(`A items: movie ${movie.name} (${movie.id}), detail page ${pageMovie.name} (${pageMovie.id}), series ${show.name} (${show.id})`)
    await ctx.snapshot("01-a-home")
    await ctx.screenshot("01-a-home")

    const media = await ctx.api(`/api/item/${movie.id}/media`)
    const source = media.body?.sources?.[0]
    if (media.status !== 200 || !source) throw new Error(`/api/item/${movie.id}/media answered ${media.status} with no source`)
    const preferenceBody = { mediaSourceIndex: 0, mediaSourceId: source.id, audioStreamIndex: source.audio[0]?.index ?? null, subtitleStreamIndex: null }
    const preferencesFile = ctx.configFile("playback-preferences.json")
    const preferencesBefore = sha256(preferencesFile)
    await ctx.unfavoriteAfterRun(movie.id)

    const uiHolds = [gate.arm("ui-media", upstream.media(pageMovie.id)), gate.arm("ui-about", upstream.about(pageMovie.id))]
    const uiRoutes = { "ui-media": `/api/item/${pageMovie.id}/media`, "ui-about": `/api/item/${pageMovie.id}/about` }
    await recordPageAnswers(ctx, Object.values(uiRoutes))
    await openByUrl(ctx, `/item/${pageMovie.id}`)
    for (const hold of uiHolds) await waitHeld(ctx, hold)

    const plan = [
      { key: "media", group: "late read", method: "GET", path: `/api/item/${movie.id}/media`, upstream: upstream.media(movie.id), expect: 409 },
      { key: "nextup", group: "late read", method: "GET", path: `/api/item/${show.id}/nextup`, upstream: upstream.seriesNextUp(show.id), expect: 409 },
      { key: "resume", group: "late read", method: "GET", path: "/api/home/resume", upstream: upstream.homeNextUp(), expect: 409 },
      { key: "trailer", group: "late read", method: "GET", path: `/api/item/${movie.id}/trailer`, upstream: upstream.trailer(movie.id), expect: 409 },
      { key: "about-401", group: "late 401", method: "GET", path: `/api/item/${movie.id}/about`, upstream: upstream.about(movie.id), hook: "answer", reply: { status: 401, body: { error: "unauthorized" } }, expect: 409 },
      { key: "playback-preference", group: "late write", method: "PATCH", path: `/api/item/${movie.id}/playback-preference`, body: preferenceBody, upstream: upstream.media(movie.id), expect: 409 },
      { key: "favorite", group: "late write", method: "POST", path: `/api/item/${movie.id}/favorite`, body: { favorite: true }, upstream: upstream.favorite(movie.id), expect: 409 },
    ]
    for (const entry of plan) {
      entry.gate = gate.arm(entry.key, entry.upstream, { hook: entry.hook, reply: entry.reply })
      await issue(ctx, entry.key, entry.method, entry.path, entry.body)
      entry.held = await waitHeld(ctx, entry.gate)
    }
    const heldNow = gate.all.filter((candidate) => candidate.arrivedAt && !candidate.releasedAt).length
    const probeStarted = Date.now()
    const probe = await ctx.app.evaluate(`Promise.race([fetch("/api/status").then((response) => response.status), new Promise((resolve) => setTimeout(() => resolve("timeout"), 5000))])`)
    report.concurrency = { held: heldNow, statusWhileHeld: probe, statusMs: Date.now() - probeStarted }
    expect(probe === 200, `/api/status still answers with ${heldNow} requests held upstream (${Date.now() - probeStarted} ms)`)
    await ctx.screenshot("02-held")

    const switchStarted = Date.now()
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.press(userMenu(a.userName, a.serverUrl))
    await ctx.press({ role: "menuitem", name: "Sign out" })
    await ctx.find(SERVER, { timeout: 30000 })
    const logAtSwitch = ctx.appLog().length
    await ctx.fill(SERVER, DEMO)
    await ctx.fill({ role: "textbox", name: "Username" }, "demo")
    await ctx.press({ role: "button", name: "Sign in" })
    await ctx.find(SETTINGS, { timeout: 60000 })
    const b = await ctx.until(async () => {
      const now = await status(ctx)
      return now?.authenticated === true && now.userName === "demo" ? now : null
    }, "/api/status to report demo", 30000)
    report.routeAfterSignIn = await ctx.pathname()
    await ctx.screenshot("02b-b-signed-in")
    if (report.routeAfterSignIn !== "/") {
      await ctx.hover({ css: '[data-sidebar="sidebar"]' })
      await ctx.press({ role: "link", name: "Home" })
    }
    await ctx.until(async () => (await ctx.pathname()) === "/" && (await ctx.count(CARD)) > 0, "B's Home with cards", 60000)
    report.concurrency.switchMs = Date.now() - switchStarted
    const settledEarly = await ctx.app.evaluate("Object.keys(window.__switchSettled ?? {})")
    expect(settledEarly.length === 0, "no held page request was answered before B's Home rendered", settledEarly)

    const logAtRelease = ctx.appLog().length
    gate.releaseAll()
    ctx.step("released every gate")
    const answers = await ctx.app.evaluate(`Promise.race([
      Promise.all(Object.entries(window.__switch).map(async ([key, answer]) => [key, await answer])).then(Object.fromEntries),
      new Promise((resolve) => setTimeout(() => resolve(null), ${RELEASE_SETTLE_MS})),
    ])`)
    if (!answers) throw new Error(`the held page requests did not settle within ${RELEASE_SETTLE_MS} ms of the release`)
    for (const entry of plan) {
      const answer = answers[entry.key]
      const flagged = markedAccountChanged(answer.body)
      const pass = entry.held && answer.status === entry.expect && answer.body.includes(STALE) && flagged
      report.cases.push({ case: entry.group, key: entry.key, route: `${entry.method} ${entry.path}`, upstream: entry.upstream.label, held: entry.held, heldAt: entry.gate.arrivedAt, releasedAt: entry.gate.releasedAt, status: answer.status, body: excerpt(answer.body), expected: `${entry.expect} ${STALE} accountChanged`, pass })
      expect(pass, `${entry.group}: ${entry.method} ${entry.path} answers ${entry.expect} with accountChanged after the switch (got ${answer.status}${flagged ? ", accountChanged" : ""})`)
    }

    const failedLines = () => [...ctx.appLog().slice(logAtRelease).matchAll(/app request failed method=(\S+) path=(\S+) status=(\d+)/g)].map((match) => ({ method: match[1], path: match[2], status: Number(match[3]) }))
    const uiHeld = uiHolds.filter((hold) => hold.arrivedAt)
    expect(uiHeld.length === uiHolds.length, `the detail page's own reads were held upstream (${uiHeld.map((hold) => hold.name).join(", ") || "none"})`)
    await ctx.until(() => uiHeld.every((hold) => failedLines().some((line) => line.path === uiRoutes[hold.name] && line.status === 409)), "app.log lines for the detail page's held reads", 10000).catch(() => null)
    await ctx.until(async () => Object.keys(await ctx.app.evaluate("window.__pageAnswers ?? {}")).length >= uiHeld.length, "the page's answers to its held reads", 10000).catch(() => null)
    const pageAnswers = await ctx.app.evaluate("window.__pageAnswers ?? {}")
    for (const hold of uiHolds) {
      const statuses = failedLines().filter((candidate) => candidate.path === uiRoutes[hold.name]).map((candidate) => candidate.status)
      const pageAnswer = pageAnswers[uiRoutes[hold.name]] ?? null
      const flagged = pageAnswer?.status === 409 && markedAccountChanged(pageAnswer.body)
      const pass = Boolean(hold.arrivedAt) && statuses.includes(409) && flagged
      const got = `${statuses.length ? statuses.join(", ") : "no app request failed line (2xx)"}; page saw ${pageAnswer ? `${pageAnswer.status}${flagged ? " accountChanged" : ""}` : "no answer"}`
      report.cases.push({ case: "late read (detail page)", key: hold.name, route: `GET ${uiRoutes[hold.name]}`, upstream: hold.label, held: Boolean(hold.arrivedAt), heldAt: hold.arrivedAt, releasedAt: hold.releasedAt, status: got, body: pageAnswer ? excerpt(pageAnswer.body) : null, expected: "409 in app.log after the release, and the page's answer 409 with accountChanged", pass })
      expect(pass, `late read (detail page): GET ${uiRoutes[hold.name]} answers 409 with accountChanged (got ${got})`)
    }

    await ctx.sleep(3000)
    const after = await status(ctx)
    report.statusAfter = { authenticated: after.authenticated, expired: after.expired, userName: after.userName }
    expect(after.authenticated === true && after.expired === false && after.userName === "demo", "late 401: B stays signed in (/api/status authenticated, not expired, demo)")
    expect(!(await ctx.exists(SERVER)) && (await ctx.exists(SETTINGS)), "late 401: the UI stays off the sign-in screen")
    expect(!ctx.appLog().slice(logAtSwitch).includes("rejected the stored token"), "late 401: app.log has no `rejected the stored token` after the switch")

    const preferencesAfter = sha256(preferencesFile)
    const preferences = existsSync(preferencesFile) ? readFileSync(preferencesFile, "utf8") : ""
    report.preferences = { before: preferencesBefore, after: preferencesAfter, mentionsMovie: preferences.includes(movie.id) }
    expect(preferencesAfter === preferencesBefore && !preferences.includes(movie.id), "late write: playback-preferences.json is unchanged and has no entry for the A movie")
    const rows = cacheRows(ctx.dataDir, movie.id)
    report.cache = { movie: movie.id, ...rows }
    expect(rows.items === 0 && rows.userData === 0, "late write: B's cache has no item or user-data row for the A movie")

    await ctx.until(async () => (await status(ctx))?.bootstrap?.state === "ready", "B's catalog to be ready", 60000)
    const absent = []
    for (const name of aTitles) if (!(await hasTitle(ctx, name))) absent.push(name)
    report.titles = { recorded: aTitles, absentOnDemo: absent }
    const home = await ctx.snapshot("03-b-home")
    await ctx.screenshot("03-b-home")
    const bRows = ((await ctx.api("/api/home")).body?.rows ?? []).filter((row) => row.items.length > 0)
    expect(bRows.length > 0 && bRows.some((row) => home.includes(row.items[0].name)), `B's Home shows B's shelves (${bRows.map((row) => row.title).join(", ")})`)
    const onHome = absent.filter((name) => home.includes(name))
    expect(onHome.length === 0, `no A title on B's Home (${absent.length} checked)`, onHome)
    let toasts = await errorToasts(ctx)
    expect(toasts.length === 0, "no error toast on B's Home", toasts)

    await ctx.press(CARD, { nth: 0 })
    await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), "a B detail page")
    await ctx.until(async () => (await ctx.count({ role: "heading" })) > 0, "the B detail page heading")
    await ctx.sleep(1000)
    const detail = await ctx.snapshot("04-b-detail")
    await ctx.screenshot("04-b-detail")
    const onDetail = absent.filter((name) => detail.includes(name))
    expect(onDetail.length === 0, `no A title on the B detail page ${await ctx.pathname()}`, onDetail)
    toasts = await errorToasts(ctx)
    expect(toasts.length === 0, "no error toast on the B detail page", toasts)

    report.held = proxy.stats.held
    report.gates = gate.all.map(({ name, label, hook, arrivedAt, releasedAt }) => ({ name, label, hook, arrivedAt, releasedAt }))
    report.signedInB = { userName: b.userName, serverUrl: b.serverUrl }

    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.press(userMenu(b.userName, b.serverUrl))
    await ctx.press({ role: "menuitem", name: "Sign out" })
    await ctx.find(SERVER, { timeout: 30000 })
    ctx.step("signed out of B")
  } finally {
    gate.releaseAll()
    ctx.writeEvidence("account-switch.json", report)
    const stored = readStoredSession(ctx.dataDir)
    if (stored?.token && stored.server_url === DEMO) {
      await revokeToken(DEMO, stored.token)
      ctx.step("signed B's token out on the demo server (the drive ended before its own sign-out)")
    }
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} account-switch checks failed:\n- ${failed.join("\n- ")}`)
}
