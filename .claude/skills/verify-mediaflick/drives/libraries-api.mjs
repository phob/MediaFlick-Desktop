const KINDS = {
  movies: "movies",
  tvshows: "shows",
  music: "music",
  musicvideos: "musicVideos",
  homevideos: "homeVideos",
  photos: "photos",
  books: "books",
  boxsets: "boxSets",
  playlists: "playlists",
  livetv: "liveTv",
  trailers: "trailers",
}
const TITLE_KIND = { movies: "Movie", shows: "Series" }
const UNKNOWN_ID = "0".repeat(32)
const CATALOG_PAGE_LIMIT = 500
const JELLYFIN_PAGE = 1000
const SAMPLE = 5

const expectedKind = (collectionType) => KINDS[collectionType?.toLowerCase()] ?? "mixed"
const sameList = (left, right) => JSON.stringify(left) === JSON.stringify(right)

async function contract(ctx, libraries, expect) {
  const status = async (path) => (await ctx.api(path)).status
  const route = (library, rest) => `/api/libraries/${library.id}/${rest}`
  const movies = libraries.find((library) => library.kind === "movies")
  const shows = libraries.find((library) => library.kind === "shows")
  const live = libraries.find((library) => !TITLE_KIND[library.kind])
  const catalog = movies ?? shows
  const result = {
    unknownId: await status(`/api/libraries/${UNKNOWN_ID}/items`),
    malformedId: await status("/api/libraries/not-a-library/items"),
    badSort: await status(route(catalog, "items?sort=sideways")),
    badLimit: await status(route(catalog, "items?limit=many")),
  }
  expect(result.unknownId === 404, `an id outside the registry answers 404 (${result.unknownId})`)
  expect(result.malformedId === 400, `a malformed id answers 400 (${result.malformedId})`)
  expect(result.badSort === 400 && result.badLimit === 400, `an unknown sort and a non-numeric limit answer 400 (${result.badSort}, ${result.badLimit})`)

  if (movies) {
    result.moviesEpisodeAdded = await status(route(movies, "items?sort=episode-added"))
    result.moviesSeriesStatus = await status(route(movies, "items?seriesStatus=Ended"))
    expect(result.moviesEpisodeAdded === 400 && result.moviesSeriesStatus === 400, `${movies.name} refuses the episode-added sort and the series status filter it does not offer (${result.moviesEpisodeAdded}, ${result.moviesSeriesStatus})`)
  }
  if (shows) {
    result.showsPlays = await status(route(shows, "items?sort=plays"))
    result.showsSeriesStatus = await status(route(shows, "items?seriesStatus=Ended&sort=episode-added"))
    expect(result.showsPlays === 400 && result.showsSeriesStatus === 200, `${shows.name} refuses the plays sort and accepts its series status filter and episode-added sort (${result.showsPlays}, ${result.showsSeriesStatus})`)
  }

  const clamped = await ctx.api(route(catalog, "items?limit=100000"))
  result.clamped = { status: clamped.status, items: clamped.body?.items?.length, total: clamped.body?.total }
  expect(clamped.status === 200 && clamped.body.items.length === Math.min(clamped.body.total, CATALOG_PAGE_LIMIT), `${catalog.name}: limit=100000 is clamped to ${CATALOG_PAGE_LIMIT} (${result.clamped.items} of ${result.clamped.total})`)

  const letters = await ctx.api(route(catalog, "letters"))
  const options = await ctx.api(route(catalog, "options"))
  const latest = await ctx.api(route(catalog, "latest"))
  result.catalogReads = {
    letters: { status: letters.status, letters: Object.keys(letters.body?.letters ?? {}).length },
    options: { status: options.status, genres: options.body?.genres?.length, officialRatings: options.body?.officialRatings?.length },
    latest: { status: latest.status, items: latest.body?.items?.length, kinds: [...new Set((latest.body?.items ?? []).map((item) => item.kind))] },
  }
  const titleKind = TITLE_KIND[catalog.kind]
  expect(letters.status === 200 && result.catalogReads.letters.letters > 0, `${catalog.name}: letters answer with ${result.catalogReads.letters.letters} letters`)
  expect(options.status === 200 && result.catalogReads.options.genres > 0, `${catalog.name}: options list ${result.catalogReads.options.genres} genres and ${result.catalogReads.options.officialRatings} ratings`)
  expect(latest.status === 200 && latest.body.items.length > 0 && sameList(result.catalogReads.latest.kinds, [titleKind]), `${catalog.name}: latest lists ${result.catalogReads.latest.items} ${titleKind} items`)

  result.letterIndex = []
  for (const library of libraries) {
    const answered = await status(route(library, "letters"))
    const expected = library.browse.letterIndex ? 200 : 400
    result.letterIndex.push({ name: library.name, kind: library.kind, letterIndex: library.browse.letterIndex, status: answered })
    expect(answered === expected, `${library.name} (${library.kind}): letters answer ${answered} with letterIndex=${library.browse.letterIndex}`)
  }

  if (live) {
    result.live = { name: live.name, kind: live.kind }
    for (const rest of ["items", "options", "latest"]) result.live[rest] = await status(route(live, rest))
    expect(["items", "options", "latest"].every((rest) => result.live[rest] === 501), `${live.name} (${live.kind}) answers 501 on its items, options and latest routes (${JSON.stringify(result.live)})`)
  } else {
    ctx.step("no library is served live on this server; the 501 answer is not exercised")
  }
  return result
}

async function appMembers(ctx, library) {
  const ids = []
  let total = null
  while (total === null || ids.length < total) {
    const page = await ctx.api(`/api/libraries/${library.id}/items?limit=${CATALOG_PAGE_LIMIT}&offset=${ids.length}`)
    if (page.status !== 200) throw new Error(`/api/libraries/${library.id}/items answered ${page.status}`)
    total = page.body.total
    if (!page.body.items.length) break
    ids.push(...page.body.items.map((item) => item.id))
  }
  return { total, ids }
}

async function jellyfinMembers(ctx, userId, library, titleKind) {
  const ids = []
  let total = null
  for (;;) {
    const page = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${library.id}&Recursive=true&IncludeItemTypes=${titleKind}&Fields=&EnableImages=false&EnableUserData=false&SortBy=DateCreated,SortName&StartIndex=${ids.length}&Limit=${JELLYFIN_PAGE}`)
    total ??= page.TotalRecordCount
    ids.push(...page.Items.map((item) => item.Id))
    if (page.Items.length < JELLYFIN_PAGE) break
  }
  return { total, ids }
}

function compareMembers(app, server) {
  const appIds = new Set(app.ids)
  const serverIds = new Set(server.ids)
  const missing = [...serverIds].filter((id) => !appIds.has(id)).sort()
  const extra = [...appIds].filter((id) => !serverIds.has(id)).sort()
  return {
    appTotal: app.total,
    appIds: app.ids.length,
    appDistinct: appIds.size,
    jellyfinTotal: server.total,
    jellyfinIds: server.ids.length,
    jellyfinDistinct: serverIds.size,
    missingFromApp: missing.length,
    missingSample: missing.slice(0, SAMPLE),
    extraInApp: extra.length,
    extraSample: extra.slice(0, SAMPLE),
    equal: missing.length === 0 && extra.length === 0 && appIds.size === app.ids.length && app.ids.length === app.total,
  }
}

export default async function librariesApi(ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)

  const report = { mode: ctx.account ? `account ${ctx.account.server}` : "demo", checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }

  const { userId } = (await ctx.api("/api/status")).body
  const listed = await ctx.api("/api/libraries")
  const views = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items
  const libraries = listed.body?.libraries ?? []
  report.libraries = libraries.map((library) => ({ id: library.id, name: library.name, kind: library.kind }))
  report.userViews = views.map((view) => ({ id: view.Id, name: view.Name, collectionType: view.CollectionType ?? null, kind: expectedKind(view.CollectionType) }))
  report.titles = listed.body?.titles ?? null
  report.membershipPending = libraries.map((library) => ({ name: library.name, kind: library.kind, membershipPending: library.membershipPending }))
  expect(listed.status === 200 && listed.body.known === true, `/api/libraries answers ${listed.status} with known=${listed.body?.known}`)
  for (const library of libraries) {
    expect(library.membershipPending === false, `${library.name} (${library.kind}): membershipPending is false after the first sync (${library.membershipPending})`)
  }
  expect(views.length > 0, `/UserViews lists ${views.length} libraries`)
  expect(
    sameList(report.libraries, report.userViews.map(({ id, name, kind }) => ({ id: id.toLowerCase(), name, kind }))),
    `ids, names, order and kinds equal /UserViews (${JSON.stringify(report.libraries.map((library) => `${library.name}:${library.kind}`))})`,
  )

  report.members = []
  for (const library of libraries.filter((candidate) => TITLE_KIND[candidate.kind])) {
    const titleKind = TITLE_KIND[library.kind]
    const app = await appMembers(ctx, library)
    const server = await jellyfinMembers(ctx, userId, library, titleKind)
    const firstKind = app.ids.length ? (await ctx.api(`/api/libraries/${library.id}/items?limit=1`)).body?.items?.[0]?.kind ?? null : null
    const entry = { id: library.id, name: library.name, kind: library.kind, titleKind, firstItemKind: firstKind, browse: library.browse, ...compareMembers(app, server) }
    report.members.push(entry)
    expect(entry.equal, `${library.name}: its grid lists the same ${entry.appDistinct} ${titleKind} ids as Jellyfin's ${entry.jellyfinDistinct} (missing ${entry.missingFromApp}, extra ${entry.extraInApp}, total ${entry.appTotal})`)
    expect(entry.appTotal === entry.jellyfinTotal, `${library.name}: total ${entry.appTotal} equals Jellyfin's ${titleKind} count ${entry.jellyfinTotal}`)
    expect(entry.appTotal === 0 || entry.firstItemKind === titleKind, `${library.name}: its grid lists ${titleKind} items (${entry.firstItemKind})`)
  }
  expect(report.members.length > 0, `${report.members.length} movies or TV shows libraries compared`)

  if (report.members.length) report.contract = await contract(ctx, libraries, expect)

  ctx.writeEvidence("libraries-report.json", report)
  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
