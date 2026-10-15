// The sidebar follows the server's library list without a reload. The test
// account must start with no playlist. Jellyfin adds a Playlists view to its
// /UserViews with the account's first playlist and drops it with the last, so
// the drive creates a private playlist from outside the app, asks for a sync
// and expects the sidebar to gain Playlists; then it deletes the playlist,
// syncs through the user menu's Sync library and expects the entry gone.
// app.log must show exactly one `libraries-changed` announcement per change
// and none for a sync that changed nothing, and the first one before the first
// catalog walk finishes, while the sidebar already lists the libraries. The
// page's own /api/libraries requests are counted per sync. The playlist is
// deleted in a finally block, and after the run if the drive failed first.
//   just verify libraries-changed --account https://jellyfin.example.com --timeout 600
const CHANGED = "announced a changed library list"
const SETTLED = "announced settled library memberships"
const BOOTSTRAPPED = "library bootstrap complete"
const SYNC_WAIT_MS = 120000
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
  expect(!shown.bootstrapped, `the sidebar listed the libraries at ${shown.at}, before app.log showed "${BOOTSTRAPPED}" (${shown.names.join(", ")})`)
  expect(!shown.names.includes("Playlists"), `the sidebar starts without Playlists (${shown.names.join(", ")})`)
  report.sidebarBefore = shown.names

  async function sidebarShot(name) {
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.screenshot(name)
    await ctx.park()
  }
  await sidebarShot("01-before")

  // Requests a sync without the user menu: opening it re-renders the
  // sidebar's list hook, which restores a lost query function and hid a
  // refetch that failed once a card had rendered.
  const requestSyncFromPage = () => ctx.app.evaluate(`fetch("/api/sync", { method: "POST" }).then((response) => response.status)`)
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
    const requests = await ctx.until(async () => {
      const made = (await librariesRequests(ctx)).slice(requestsBefore)
      return made.length >= expected && made.every((request) => request.status !== null) ? made : null
    }, `the page's /api/libraries requests after ${label}`, 10000).catch(async () => (await librariesRequests(ctx)).slice(requestsBefore))
    const cycle = { label, before, after, changed, settled, requests }
    report.cycles.push(cycle)
    return cycle
  }

  // Each announcement invalidates the list once, and an invalidation that
  // lost its query function sends no request at all.
  const expectOneReadPerAnnouncement = (cycle) => expect(
    cycle.requests.length === cycle.changed + cycle.settled && cycle.requests.every((request) => request.status === 200),
    `${cycle.label}: the page read /api/libraries once per announcement (${cycle.changed} changed + ${cycle.settled} settled, requests ${JSON.stringify(cycle.requests)})`,
  )

  const quiet = await sync("an unchanged library list", requestSyncFromMenu)
  await ctx.park()
  expect(quiet.changed === 0, `a sync that changes no library announces nothing (${quiet.changed})`)
  expect(quiet.settled >= 1, `the requested sync's membership walks announced that they settled (${quiet.settled})`)
  expectOneReadPerAnnouncement(quiet)

  const movies = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items.find((view) => view.CollectionType === "movies")
  if (!movies) throw new Error("libraries-changed needs a movies library to hover a card in")
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ css: `[data-sidebar="group"] a[href="/library/${movies.Id}"]` })
  await ctx.until(async () => (await ctx.pathname()).startsWith(`/library/${movies.Id}`), `the ${movies.Name} library page`)
  await ctx.park()
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
    const added = await sync("the new Playlists view, with a card preview open", async () => {
      report.pageSyncStatus = await requestSyncFromPage()
    })
    report.sidebarDuring = await ctx.until(async () => {
      const names = await sidebarLibraries(ctx)
      return names?.includes("Playlists") ? names : null
    }, "the sidebar to list Playlists", 30000).catch(() => null)
    expect(report.pageSyncStatus === 200, `the sync was requested without the sidebar (${report.pageSyncStatus})`)
    expect(report.sidebarDuring !== null, `the sidebar gains Playlists after a sync that followed a card preview, with no sidebar interaction (${JSON.stringify(report.sidebarDuring ?? await sidebarLibraries(ctx))})`)
    expect(added.changed === 1, `the added view announced the list exactly once (${added.changed})`)
    expectOneReadPerAnnouncement(added)
    expect(await ctx.app.evaluate("window.__librariesChangedNoReload === true"), "the page was not reloaded")
    await ctx.park()
    await sidebarShot("02-during")

    const steady = await sync("the list holding Playlists", requestSyncFromMenu)
    await ctx.park()
    expect(steady.changed === 0, `a second sync with Playlists unchanged announces nothing (${steady.changed})`)
    expectOneReadPerAnnouncement(steady)

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
