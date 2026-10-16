// A private playlist changed on the server while its page is open. The drive
// creates a playlist of three movies out of name order as the test account,
// opens its page and expects the cards in the playlist's own order. Then, as
// another device would, it adds a fourth movie, moves it to the second place
// and adds the first movie a second time, and expects the open page to follow
// without a reload once Jellyfin pushes the change over its socket, with both
// cards of the repeated movie in their slots. Jellyfin's PlaylistItemId is the
// item's own id, so the app numbers slots itself (playlistIndex).
// Jellyfin reports a playlist change as LibraryChanged ItemsUpdated
// [<playlist>], about 30 s after the server's last library change. The
// playlist is deleted at the end, and after the run if the drive failed first.
// With --signed-out the app signs in through a loopback proxy, which then cuts
// the app's socket while the playlist changes again and restores it only after
// Jellyfin's push has gone out to nobody. The open page must follow once the
// app reconnects, though no push names the playlist.
//   just verify live-changes --account https://jellyfin.example.com --timeout 600
//   just verify live-changes --account https://jellyfin.example.com --signed-out --timeout 900
const GRID_CARD_LINK = '[data-item-grid-canvas] article a[aria-label^="Open details for"]'
const PUSH_WAIT_MS = 240000
const LIVE_NOTICE = "announced a pushed change to items the cache does not hold"
const RECONNECT_NOTICE = "the event stream reconnected"
const SOCKET_CUT_MS = 90000
const RECONNECT_WAIT_MS = 240000
const AFTER_RECONNECT_MS = 30000

async function openByUrl(ctx, path) {
  await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(path)}); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(async () => (await ctx.pathname()) === path, `the ${path} route`)
}

async function cardIds(ctx) {
  return ctx.app.evaluate(`[...document.querySelectorAll(${JSON.stringify(GRID_CARD_LINK)})]
    .map((link) => ({ rect: link.getBoundingClientRect(), id: decodeURIComponent(link.getAttribute("href").replace(/^.*\\/item\\//, "")) }))
    .sort((left, right) => left.rect.top - right.rect.top || left.rect.left - right.rect.left)
    .map((card) => card.id)`)
}

const sameList = (left, right) => JSON.stringify(left) === JSON.stringify(right)

async function changeWhileDisconnected(ctx, proxy, playlist, movedId, expect) {
  const result = {}
  const logBefore = ctx.appLog().length
  const upgradesBefore = proxy.stats.upgrades
  proxy.cutSockets()
  await ctx.until(() => proxy.stats.refusedUpgrades > 0, "the app to try to reopen its socket", 60000)
  const entries = await playlist.items()
  const changedAt = Date.now()
  await playlist.move(entries.find((item) => item.Id === movedId).PlaylistItemId, entries.length - 1)
  const changed = (await playlist.items()).map((item) => item.Id)
  result.server = changed
  await new Promise((resolve) => setTimeout(resolve, SOCKET_CUT_MS))
  result.cardsWhileCut = await cardIds(ctx)
  expect(!sameList(result.cardsWhileCut, changed), `the page did not learn of the change while the socket was cut (${JSON.stringify(result.cardsWhileCut)})`)
  proxy.restoreSockets()
  const reconnected = await ctx.until(() => proxy.stats.upgrades > upgradesBefore, "the app to reopen its socket", RECONNECT_WAIT_MS).then(() => true, () => false)
  result.secondsCut = Math.round((Date.now() - changedAt) / 1000)
  expect(reconnected, `the app reopened its socket ${result.secondsCut} s after the change (${proxy.stats.refusedUpgrades} attempts refused)`)
  const followed = reconnected && await ctx.until(async () => sameList(await cardIds(ctx), changed), "the open page to show the order changed while disconnected", AFTER_RECONNECT_MS).then(() => true, () => false)
  result.cards = await cardIds(ctx)
  const lines = ctx.appLog().slice(logBefore).split("\n")
  result.pushes = lines.filter((line) => line.includes(LIVE_NOTICE) && line.includes(playlist.id))
  result.reconnects = lines.filter((line) => line.includes(RECONNECT_NOTICE))
  expect(followed, `the open page shows the order changed while disconnected within ${AFTER_RECONNECT_MS / 1000} s of the reconnect (${JSON.stringify(result.cards)})`)
  expect(result.pushes.length === 0, `no push named the playlist after the cut, so the reconnect refreshed the page (${result.pushes.length} pushes)`)
  expect(result.reconnects.length === 1, `app.log announces the reconnect once (${result.reconnects.length} lines)`)
  expect(await ctx.app.evaluate("window.__liveChangesNoReload === true"), "the page was not reloaded across the reconnect")
  await ctx.screenshot("03-after-reconnect")
  return result
}

export default async function liveChanges(ctx) {
  if (!ctx.account) throw new Error("live-changes writes a playlist and needs --account SERVER")
  const proxy = ctx.account.signedOut ? await ctx.proxy({ upstream: ctx.account.server }) : null
  if (proxy) {
    await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
    await ctx.quickConnect({ approve: true, timeout: 60000 })
    ctx.revokeAppSessionAfterRun()
  } else {
    await ctx.signIn({})
  }
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  await ctx.until(() => ctx.appLog().includes("listening for Jellyfin server events"), "the app's Jellyfin socket", 60000)

  const report = { server: ctx.account.server, throughProxy: Boolean(proxy), checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }

  const { userId } = ctx.account
  const movies = (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Movie&SortBy=SortName&Limit=4&Fields=&EnableImages=false`)).Items
  if (movies.length < 4) throw new Error(`live-changes needs four movies; the account sees ${movies.length}`)
  report.movies = movies.map((movie) => ({ id: movie.Id, name: movie.Name }))

  const [first, second, third, fourth] = movies.map((movie) => movie.Id)
  const playlist = await ctx.createPlaylistAfterRun(`MediaFlick verify ${new Date().toISOString()}`, [third, first, second])
  report.playlist = playlist.id
  try {
    const before = (await playlist.items()).map((item) => item.Id)
    report.before = { server: before }
    expect(sameList(before, [third, first, second]), `the server keeps the playlist out of name order (${JSON.stringify(before)})`)
    const detail = await ctx.api(`/api/item/${playlist.id}`)
    expect(detail.status === 200 && detail.body?.layout === "generic" && detail.body.kind === "Playlist", `the playlist answers the generic layout (${detail.status}, ${detail.body?.layout}, ${detail.body?.kind})`)

    await openByUrl(ctx, `/item/${playlist.id}`)
    await ctx.find({ role: "heading", name: "Contents" }, { timeout: 30000 })
    const shown = await ctx.until(async () => {
      const ids = await cardIds(ctx)
      return ids.length === before.length ? ids : null
    }, "the playlist's cards", 30000)
    report.before.cards = shown
    expect(sameList(shown, before), `the cards follow the playlist's own order (${JSON.stringify(shown)})`)
    await ctx.screenshot("01-before")

    await ctx.app.evaluate("window.__liveChangesNoReload = true")
    const logBefore = ctx.appLog().length
    const changedAt = Date.now()
    await playlist.add([fourth])
    const entry = (await playlist.items()).find((item) => item.Id === fourth)
    await playlist.move(entry.PlaylistItemId, 1)
    await playlist.add([first])
    const after = (await playlist.items()).map((item) => item.Id)
    report.after = { server: after }
    expect(sameList(after, [third, fourth, first, second, first]), `the server's order puts the added movie second and the repeated one last (${JSON.stringify(after)})`)

    const followed = await ctx.until(async () => sameList(await cardIds(ctx), after), "the open page to show the server's new order", PUSH_WAIT_MS).then(() => true, () => false)
    report.after.cards = await cardIds(ctx)
    report.after.secondsAfterChange = Math.round((Date.now() - changedAt) / 1000)
    const notices = ctx.appLog().slice(logBefore).split("\n").filter((line) => line.includes(LIVE_NOTICE))
    report.after.notices = notices
    expect(followed, `the open page shows the server's new order ${report.after.secondsAfterChange} s after the change (${JSON.stringify(report.after.cards)})`)
    expect(notices.some((line) => line.includes(playlist.id)), `app.log shows the pushed live change naming the playlist (${notices.length} notices)`)
    expect(await ctx.app.evaluate("window.__liveChangesNoReload === true"), "the page was not reloaded")
    expect((await ctx.pathname()) === `/item/${playlist.id}`, "the playlist page stayed open")
    await ctx.screenshot("02-after")

    const children = (await ctx.api(`/api/item/${playlist.id}/children`)).body?.items ?? []
    const slots = children.map((item) => [item.id, item.playlistIndex])
    report.after.slots = slots
    expect(sameList(slots, after.map((id, index) => [id, index])), `the children answer numbers each slot in the server's order (${JSON.stringify(slots)})`)

    if (proxy) {
      report.reconnect = await changeWhileDisconnected(ctx, proxy, playlist, fourth, expect)
    } else {
      ctx.step("the reconnect phase needs --signed-out, which signs the app in through a proxy")
    }
  } finally {
    await playlist.remove()
    const gone = !(await playlist.exists())
    report.deleted = gone
    expect(gone, `the playlist ${playlist.id} is gone from the server`)
    ctx.writeEvidence("live-changes.json", report)
  }

  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
