// The sidebar follows the server's library list without a reload. The test
// account must start with no playlist. Jellyfin adds a Playlists view to its
// /UserViews with the account's first playlist and drops it with the last, so
// the drive creates a private playlist from outside the app, asks for a sync
// through the user menu's Sync library, and expects the sidebar to gain
// Playlists; then it deletes the playlist, syncs again and expects the entry
// gone. app.log must show exactly one `libraries-changed` announcement per
// change and none for a sync that changed nothing. The playlist is deleted in
// a finally block, and after the run if the drive failed first.
//   just verify libraries-changed --account https://jellyfin.example.com --timeout 600
const CHANGED = "announced a changed library list"
const SETTLED = "announced settled library memberships"
const SYNC_WAIT_MS = 120000

const occurrences = (text, needle) => text.split(needle).length - 1

const sidebarLibraries = (ctx) => ctx.app.evaluate(`(() => {
  const group = [...document.querySelectorAll('[data-sidebar="group"]')]
    .find((candidate) => candidate.querySelector('[data-sidebar="group-label"]')?.textContent === "Libraries")
  return group ? [...group.querySelectorAll('a[href^="/library/"]')].map((link) => link.textContent.trim()) : null
})()`)

async function playlistsOf(ctx, userId) {
  const items = await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Playlist&Fields=&EnableImages=false`)
  const views = await ctx.jellyfin(`/UserViews?userId=${userId}`)
  return {
    playlists: items.Items.map((item) => ({ id: item.Id, name: item.Name })),
    playlistViews: views.Items.filter((view) => view.CollectionType === "playlists").map((view) => view.Name),
  }
}

export default async function librariesChanged(ctx) {
  if (!ctx.account) throw new Error("libraries-changed writes a playlist and needs --account SERVER")
  await ctx.signIn({})
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

  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false && !status.syncing
  }, "the first sync to finish", 300000)
  const status = (await ctx.api("/api/status")).body
  const userMenu = { role: "button", name: `${status.userName.slice(0, 1)} ${status.userName} ${new URL(status.serverUrl).host}` }

  async function sync(label) {
    await ctx.until(async () => !(await ctx.api("/api/status")).body?.syncing, `no sync running before ${label}`, SYNC_WAIT_MS)
    const before = { ...counts(), lastSync: (await ctx.api("/api/status")).body?.lastSync ?? null, sidebar: await sidebarLibraries(ctx) }
    await ctx.hover(userMenu)
    await ctx.press(userMenu)
    await ctx.press({ role: "menuitem", name: "Sync library" })
    await ctx.until(async () => {
      const now = (await ctx.api("/api/status")).body
      return !now?.syncing && (counts().settled > before.settled || (now?.lastSync ?? null) !== before.lastSync)
    }, `the sync requested for ${label} to finish`, SYNC_WAIT_MS)
    await ctx.park()
    const after = counts()
    const cycle = { label, before, after, changed: after.changed - before.changed, settled: after.settled - before.settled }
    report.cycles.push(cycle)
    return cycle
  }

  async function sidebarShot(name) {
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.screenshot(name)
    await ctx.park()
  }

  report.firstRead = counts()
  expect(report.firstRead.changed === 1, `the first library read announced the list once (${report.firstRead.changed})`)
  const initial = await ctx.until(async () => {
    const names = await sidebarLibraries(ctx)
    return names?.length ? names : null
  }, "the sidebar's Libraries group")
  report.sidebarBefore = initial
  expect(!initial.includes("Playlists"), `the sidebar starts without Playlists (${initial.join(", ")})`)
  await sidebarShot("01-before")

  const quiet = await sync("an unchanged library list")
  expect(quiet.changed === 0, `a sync that changes no library announces nothing (${quiet.changed})`)
  expect(quiet.settled >= 1, `the requested sync's membership walks announced that they settled (${quiet.settled})`)

  await ctx.app.evaluate("window.__librariesChangedNoReload = true")
  const movie = (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Movie&SortBy=SortName&Limit=1&Fields=&EnableImages=false`)).Items[0]
  if (!movie) throw new Error("libraries-changed needs one movie for its playlist")
  const playlist = await ctx.createPlaylistAfterRun(`MediaFlick verify ${new Date().toISOString()}`, [movie.Id])
  report.playlist = playlist.id
  try {
    report.created = await playlistsOf(ctx, userId)
    report.measuredView = report.created.playlistViews.length === 1
    expect(report.measuredView, `Jellyfin lists a Playlists view once the account has a playlist (${JSON.stringify(report.created.playlistViews)})`)

    const added = await sync("the new Playlists view")
    report.sidebarDuring = await ctx.until(async () => {
      const names = await sidebarLibraries(ctx)
      return names?.includes("Playlists") ? names : null
    }, "the sidebar to list Playlists", 30000).catch(() => null)
    expect(report.sidebarDuring !== null, `the sidebar gains Playlists after the sync (${JSON.stringify(report.sidebarDuring ?? await sidebarLibraries(ctx))})`)
    expect(added.changed === 1, `the added view announced the list exactly once (${added.changed})`)
    expect(await ctx.app.evaluate("window.__librariesChangedNoReload === true"), "the page was not reloaded")
    await sidebarShot("02-during")

    const steady = await sync("the list holding Playlists")
    expect(steady.changed === 0, `a second sync with Playlists unchanged announces nothing (${steady.changed})`)

    await playlist.remove()
    report.deleted = await playlistsOf(ctx, userId)
    expect(report.deleted.playlistViews.length === 0, `Jellyfin drops the Playlists view with the last playlist (${JSON.stringify(report.deleted.playlistViews)})`)
    const removed = await sync("the removed Playlists view")
    report.sidebarAfter = await ctx.until(async () => {
      const names = await sidebarLibraries(ctx)
      return names && !names.includes("Playlists") ? names : null
    }, "the sidebar to drop Playlists", 30000).catch(() => null)
    expect(report.sidebarAfter !== null, `the sidebar drops Playlists after the sync (${JSON.stringify(report.sidebarAfter ?? await sidebarLibraries(ctx))})`)
    expect(removed.changed === 1, `the removed view announced the list exactly once (${removed.changed})`)
    expect(await ctx.app.evaluate("window.__librariesChangedNoReload === true"), "the page was still not reloaded")
    await sidebarShot("03-after")
  } finally {
    if (await playlist.exists()) await playlist.remove()
    report.end = await playlistsOf(ctx, userId)
    report.accountAsFound = report.end.playlists.length === 0 && report.end.playlistViews.length === 0
    expect(report.accountAsFound, `the account ends as found, with no playlist and no Playlists view (${JSON.stringify(report.end)})`)
    report.total = counts()
    ctx.writeEvidence("libraries-changed.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed:\n${failed.map((check) => check.message).join("\n")}`)
}
