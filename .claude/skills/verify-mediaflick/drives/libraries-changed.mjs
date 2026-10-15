// The sidebar follows the server's library list without a reload. The test
// account must start with no playlist. Jellyfin adds a Playlists view to its
// /UserViews with the account's first playlist and drops it with the last, so
// the drive creates a private playlist from outside the app, asks for a sync
// and expects the sidebar to gain Playlists; then it deletes the playlist,
// syncs through the user menu's Sync library and expects the entry gone.
// app.log must show exactly one `libraries-changed` announcement per change
// and none for a sync that changed nothing, and the first one before the first
// catalog download finishes. While that download runs, /api/libraries must
// report every movies or TV shows library as pending, and an open library page
// must not say "Nothing to show" before its titles arrive. The page's own
// /api/libraries requests are counted per sync, and no query in the page's
// cache may fail. The playlist is deleted in a finally block, and after the
// run if the drive failed first.
//   just verify libraries-changed --account https://jellyfin.example.com --timeout 600
const CHANGED = "announced a changed library list"
const SETTLED = "announced settled library memberships"
const BOOTSTRAPPED = "library bootstrap complete"
const SYNC_WAIT_MS = 120000
const SETTLE_MS = 2000
const CARD = 'a[aria-label^="Open details for "]'
const PREVIEW_LINK = '.preview-panel a[aria-label^="Open details for "]'
// Counts the page's own /api/libraries requests. It never sends one.
const LIBRARIES_REQUESTS = `(() => {
  if (window.__librariesRequests) return
  window.__librariesRequests = []
  const original = window.fetch
  window.fetch = async function (input, init) {
    const url = input instanceof Request ? input.url : String(input)
    if (new URL(url, location.href).pathname !== "/api/libraries") return original.apply(this, arguments)
    const request = { at: new Date().toISOString(), status: null }
    window.__librariesRequests.push(request)
    try {
      const response = await original.apply(this, arguments)
      request.status = response.status
      return response
    } catch (error) {
      request.status = String(error)
      throw error
    }
  }
})()`

const occurrences = (text, needle) => text.split(needle).length - 1

const sidebarLibraries = (ctx) => ctx.app.evaluate(`(() => {
  const group = [...document.querySelectorAll('[data-sidebar="group"]')]
    .find((candidate) => candidate.querySelector('[data-sidebar="group-label"]')?.textContent === "Libraries")
  return group ? [...group.querySelectorAll('a[href^="/library/"]')].map((link) => link.textContent.trim()) : null
})()`)

const librariesRequests = (ctx) => ctx.app.evaluate("window.__librariesRequests.slice()")

// Reads the page's React Query cache through the React root: every failed
// query, and the state of each library list and library options query. It
// changes nothing.
const queryCache = (ctx) => ctx.app.evaluate(`(() => {
  const root = document.getElementById("root")
  const stack = [root[Object.keys(root).find((key) => key.startsWith("__reactContainer$"))]]
  while (stack.length) {
    const fiber = stack.pop()
    if (!fiber) continue
    const client = fiber.memoizedProps?.client
    if (client?.getQueryCache) {
      const queries = client.getQueryCache().getAll()
      return {
        failed: queries
          .filter((query) => query.state.status === "error")
          .map((query) => ({ key: query.queryKey, error: String(query.state.error?.message ?? query.state.error) })),
        libraryQueries: queries
          .filter((query) => query.queryKey[0] === "libraries" || query.queryKey[0] === "library-options")
          .map((query) => ({
            key: query.queryKey,
            status: query.state.status,
            fetchStatus: query.state.fetchStatus,
            fetches: query.state.dataUpdateCount + query.state.errorUpdateCount,
            observers: query.getObserversCount(),
            active: query.isActive(),
          })),
      }
    }
    stack.push(fiber.sibling, fiber.child)
  }
  return null
})()`)

const gridState = (ctx) => ctx.app.evaluate(`document.body.innerText.includes("Nothing to show") ? "empty" : document.querySelector(${JSON.stringify(CARD)}) ? "titles" : "placeholder"`)

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const isCatalog = (library) => library.kind === "movies" || library.kind === "shows"

async function openLibrary(ctx, library) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ css: `[data-sidebar="group"] a[href="/library/${library.id}"]` })
  await ctx.until(async () => (await ctx.pathname()).startsWith(`/library/${library.id}`), `the ${library.name} library page`)
  await ctx.park()
}

// Opens the movies or TV shows library holding the fewest titles while the
// first catalog download runs, then samples /api/libraries and that grid
// until the download is complete and the grid shows titles. app.log is read
// after each answer, so a sample counts as taken before the download finished
// only if it really was.
async function watchFirstFill(ctx) {
  const { libraries } = (await ctx.api("/api/libraries")).body
  const catalog = libraries.filter(isCatalog)
  const totals = []
  for (const library of catalog) {
    totals.push({ id: library.id, name: library.name, total: (await ctx.api(`/api/libraries/${library.id}/items?limit=1`)).body?.total ?? null })
  }
  const target = [...totals].sort((left, right) => (left.total ?? 0) - (right.total ?? 0))[0]
  const fill = { catalog: totals, target, pending: [], grid: [] }
  if (!target) return fill
  await openLibrary(ctx, catalog.find((library) => library.id === target.id))
  fill.openedAt = new Date().toISOString()
  fill.openedTotal = (await ctx.api(`/api/libraries/${target.id}/items?limit=1`)).body?.total ?? null
  fill.openedBeforeBootstrap = !ctx.appLog().includes(BOOTSTRAPPED)
  const deadline = Date.now() + 120000
  while (Date.now() < deadline) {
    const listed = (await ctx.api("/api/libraries")).body
    fill.pending.push({
      at: new Date().toISOString(),
      bootstrapped: ctx.appLog().includes(BOOTSTRAPPED),
      pending: Object.fromEntries((listed?.libraries ?? []).filter(isCatalog).map((library) => [library.name, library.membershipPending])),
    })
    const state = await gridState(ctx)
    fill.grid.push({ at: new Date().toISOString(), state, bootstrapped: ctx.appLog().includes(BOOTSTRAPPED) })
    if (fill.pending.at(-1).bootstrapped && state === "titles") break
    await sleep(250)
  }
  return fill
}
async function playlistsOf(ctx, userId) {
  const items = await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Playlist&Fields=&EnableImages=false`)
  const views = await ctx.jellyfin(`/UserViews?userId=${userId}`)
  return {
    playlists: items.Items.map((item) => ({ id: item.Id, name: item.Name })),
    playlistViews: views.Items.filter((view) => view.CollectionType === "playlists").map((view) => view.Name),
  }
}

function logLine(log, needle) {
  const lines = log.split("\n")
  const index = lines.findIndex((line) => line.includes(needle))
  return index < 0 ? null : { index, at: lines[index].slice(0, 27) }
}

export default async function librariesChanged(ctx) {
  if (!ctx.account) throw new Error("libraries-changed writes a playlist and needs --account SERVER")
  await ctx.signIn({})
  await ctx.app.send("Page.addScriptToEvaluateOnNewDocument", { source: LIBRARIES_REQUESTS })
  await ctx.app.evaluate(LIBRARIES_REQUESTS)
  const { userId } = ctx.account
  const report = { server: ctx.account.server, checks: [], cycles: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const counts = () => {
    const log = ctx.appLog()
    return { changed: occurrences(log, CHANGED), settled: occurrences(log, SETTLED) }
  }

  report.start = await playlistsOf(ctx, userId)
  if (report.start.playlists.length || report.start.playlistViews.length) {
    ctx.writeEvidence("libraries-changed.json", report)
    throw new Error(`the test account must start without playlists; it has ${JSON.stringify(report.start)}`)
  }

  const shown = await ctx.until(async () => {
    const names = await sidebarLibraries(ctx)
    return names?.length ? { names, at: new Date().toISOString(), bootstrapped: ctx.appLog().includes(BOOTSTRAPPED) } : null
  }, "the sidebar's Libraries group", 300000)
  report.firstShown = shown
  await ctx.screenshot("00-first-read")
  const fill = await watchFirstFill(ctx)
  report.firstFill = fill
  await ctx.screenshot("00b-first-fill")
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false && !status.syncing
  }, "the first sync to finish", 300000)
  const status = (await ctx.api("/api/status")).body
  const userMenu = { role: "button", name: `${status.userName.slice(0, 1)} ${status.userName} ${new URL(status.serverUrl).host}` }

  const firstLog = ctx.appLog()
  report.firstRead = {
    ...counts(),
    announced: logLine(firstLog, CHANGED),
    bootstrapped: logLine(firstLog, BOOTSTRAPPED),
  }
  expect(report.firstRead.changed === 1, `the first library read announced the list once (${report.firstRead.changed})`)
  expect(
    report.firstRead.bootstrapped !== null && report.firstRead.announced !== null && report.firstRead.announced.index < report.firstRead.bootstrapped.index,
    `app.log announces the first list before the first catalog walk completes (${JSON.stringify({ announced: report.firstRead.announced?.at, bootstrapped: report.firstRead.bootstrapped?.at })})`,
  )
  const whileDownloading = fill.pending.filter((sample) => !sample.bootstrapped)
  expect(
    whileDownloading.length > 0 && whileDownloading.every((sample) => Object.values(sample.pending).length > 0 && Object.values(sample.pending).every(Boolean)),
    `/api/libraries reported every movies or TV shows library as pending in each of ${whileDownloading.length} reads before "${BOOTSTRAPPED}" (${JSON.stringify(whileDownloading.map((sample) => sample.pending))})`,
  )
  const firstTitles = fill.grid.findIndex((sample) => sample.state === "titles")
  const beforeTitles = firstTitles < 0 ? fill.grid : fill.grid.slice(0, firstTitles)
  fill.exercised = fill.openedTotal === 0 && fill.openedBeforeBootstrap
  expect(firstTitles >= 0, `${fill.target?.name}: the grid opened during the first download shows titles once they arrive (${fill.grid.at(-1)?.state})`)
  expect(
    beforeTitles.every((sample) => sample.state === "placeholder"),
    `${fill.target?.name}: opened during the first download with ${fill.openedTotal} titles, the grid showed its placeholder in ${beforeTitles.length} samples and never "Nothing to show" before its titles arrived (${[...new Set(beforeTitles.map((sample) => sample.state))].join(", ") || "titles at once"})`,
  )
  ctx.step(`first fill: ${fill.exercised ? "the grid was opened while the library held no titles" : "the library already held titles when its grid opened, so the grid check proves nothing on this run"}`)
  expect(!shown.names.includes("Playlists"), `the sidebar starts without Playlists (${shown.names.join(", ")})`)
  report.sidebarBefore = shown.names

  async function sidebarShot(name) {
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.screenshot(name)
    await ctx.park()
  }
  await sidebarShot("01-before")

  const requestSyncFromMenu = async () => {
    await ctx.hover(userMenu)
    await ctx.press(userMenu)
    await ctx.press({ role: "menuitem", name: "Sync library" })
  }

  async function sync(label, request) {
    await ctx.until(async () => !(await ctx.api("/api/status")).body?.syncing, `no sync running before ${label}`, SYNC_WAIT_MS)
    const requestsBefore = (await librariesRequests(ctx)).length
    const before = { ...counts(), lastSync: (await ctx.api("/api/status")).body?.lastSync ?? null }
    await request()
    await ctx.until(async () => {
      const now = (await ctx.api("/api/status")).body
      return !now?.syncing && (counts().settled > before.settled || (now?.lastSync ?? null) !== before.lastSync)
    }, `the sync requested for ${label} to finish`, SYNC_WAIT_MS)
    const after = counts()
    const changed = after.changed - before.changed
    const settled = after.settled - before.settled
    const expected = changed + settled
    await ctx.until(async () => {
      const made = (await librariesRequests(ctx)).slice(requestsBefore)
      return made.length >= expected && made.every((request) => request.status !== null)
    }, `the page's /api/libraries requests after ${label}`, 10000).catch(() => null)
    await sleep(SETTLE_MS)
    const requests = (await librariesRequests(ctx)).slice(requestsBefore)
    const cycle = { label, before, after, changed, settled, requests }
    report.cycles.push(cycle)
    return cycle
  }

  // Each announcement invalidates the list once, and an invalidation that
  // lost its query function sends no request at all. The count is taken
  // SETTLE_MS after it reached the expected value, so an extra read fails too.
  const expectOneReadPerAnnouncement = (cycle) => expect(
    cycle.requests.length === cycle.changed + cycle.settled && cycle.requests.every((request) => request.status === 200),
    `${cycle.label}: ${SETTLE_MS} ms after the sync, the page had read /api/libraries exactly once per announcement (${cycle.changed} changed + ${cycle.settled} settled, requests ${JSON.stringify(cycle.requests)})`,
  )

  const quiet = await sync("an unchanged library list", requestSyncFromMenu)
  await ctx.park()
  expect(quiet.changed === 0, `a sync that changes no library announces nothing (${quiet.changed})`)
  expect(quiet.settled >= 1, `the requested sync's membership walks announced that they settled (${quiet.settled})`)
  expectOneReadPerAnnouncement(quiet)

  const listed = (await ctx.api("/api/libraries")).body.libraries
  const movies = listed.find((library) => library.kind === "movies")
  if (!movies) throw new Error("libraries-changed needs a movies library to hover a card in")
  const unfiltered = listed.find((library) => !library.browse.filters.includes("genre") && !library.browse.filters.includes("parental"))
  if (!unfiltered) throw new Error("libraries-changed needs a library without genre or parental filters for its query check")
  await openLibrary(ctx, movies)
  await ctx.find({ css: CARD }, { nth: 0, timeout: 30000 })

  await ctx.app.evaluate("window.__librariesChangedNoReload = true")
  const movie = (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Movie&SortBy=SortName&Limit=1&Fields=&EnableImages=false`)).Items[0]
  if (!movie) throw new Error("libraries-changed needs one movie for its playlist")
  const playlist = await ctx.createPlaylistAfterRun(`MediaFlick verify ${new Date().toISOString()}`, [movie.Id])
  report.playlist = playlist.id
  try {
    report.created = await playlistsOf(ctx, userId)
    report.measuredView = report.created.playlistViews.length === 1
    expect(report.measuredView, `Jellyfin lists a Playlists view once the account has a playlist (${JSON.stringify(report.created.playlistViews)})`)

    await ctx.hover({ css: CARD }, { nth: 0 })
    await ctx.find({ css: PREVIEW_LINK }, { timeout: 10000 })
    report.hoveredCard = await ctx.app.evaluate(`document.querySelector(${JSON.stringify(PREVIEW_LINK)}).getAttribute("aria-label")`)
    const added = await sync("the new Playlists view, after a card preview", requestSyncFromMenu)
    report.sidebarDuring = await ctx.until(async () => {
      const names = await sidebarLibraries(ctx)
      return names?.includes("Playlists") ? names : null
    }, "the sidebar to list Playlists", 30000).catch(() => null)
    expect(report.sidebarDuring !== null, `the sidebar gains Playlists after a sync that followed a card preview (${JSON.stringify(report.sidebarDuring ?? await sidebarLibraries(ctx))})`)
    expect(added.changed === 1, `the added view announced the list exactly once (${added.changed})`)
    expectOneReadPerAnnouncement(added)
    expect(await ctx.app.evaluate("window.__librariesChangedNoReload === true"), "the page was not reloaded")
    await ctx.park()
    await sidebarShot("02-during")

    const steady = await sync("the list holding Playlists", requestSyncFromMenu)
    await ctx.park()
    expect(steady.changed === 0, `a second sync with Playlists unchanged announces nothing (${steady.changed})`)
    expectOneReadPerAnnouncement(steady)

    await openLibrary(ctx, unfiltered)
    report.unfiltered = unfiltered.name
    await playlist.remove()
    report.deleted = await playlistsOf(ctx, userId)
    expect(report.deleted.playlistViews.length === 0, `Jellyfin drops the Playlists view with the last playlist (${JSON.stringify(report.deleted.playlistViews)})`)
    const removed = await sync("the removed Playlists view", requestSyncFromMenu)
    await ctx.park()
    report.sidebarAfter = await ctx.until(async () => {
      const names = await sidebarLibraries(ctx)
      return names && !names.includes("Playlists") ? names : null
    }, "the sidebar to drop Playlists", 30000).catch(() => null)
    expect(report.sidebarAfter !== null, `the sidebar drops Playlists after the sync (${JSON.stringify(report.sidebarAfter ?? await sidebarLibraries(ctx))})`)
    expect(removed.changed === 1, `the removed view announced the list exactly once (${removed.changed})`)
    expectOneReadPerAnnouncement(removed)
    expect(await ctx.app.evaluate("window.__librariesChangedNoReload === true"), "the page was still not reloaded")
    report.queryCache = await queryCache(ctx)
    expect(
      Array.isArray(report.queryCache?.failed) && report.queryCache.failed.length === 0,
      `on ${unfiltered.name}, which lists no genre or parental filter, no query in the page's cache failed after the list changed (${JSON.stringify(report.queryCache?.failed)}; library queries ${JSON.stringify(report.queryCache?.libraryQueries)})`,
    )
    await sidebarShot("03-after")
  } finally {
    if (await playlist.exists()) await playlist.remove()
    report.end = await playlistsOf(ctx, userId)
    report.accountAsFound = report.end.playlists.length === 0 && report.end.playlistViews.length === 0
    expect(report.accountAsFound, `the account ends as found, with no playlist and no Playlists view (${JSON.stringify(report.end)})`)
    report.total = counts()
    report.librariesRequests = await librariesRequests(ctx).catch(() => null)
    ctx.writeEvidence("libraries-changed.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed:\n${failed.map((check) => check.message).join("\n")}`)
}
