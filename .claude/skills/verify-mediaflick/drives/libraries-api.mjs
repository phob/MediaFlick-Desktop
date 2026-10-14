// /api/libraries lists exactly the libraries Jellyfin's /UserViews lists for
// the signed-in user, in its order, each with the kind its CollectionType
// names, and each movies or TV shows library's grid total equals Jellyfin's
// own count of that library's movies or series.
// Also the library routes' contract: an id outside the registry is a 404, a
// malformed id, parameter, sort or filter the library does not offer a 400,
// a page limit is clamped, and a library served live answers 501.
// Read-only in both modes.
//   just verify libraries-api --url https://demo.jellyfin.org/stable
//   just verify libraries-api --account https://jellyfin.example.com
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

  if (live) {
    result.live = { name: live.name, kind: live.kind }
    for (const rest of ["items", "letters", "options", "latest"]) result.live[rest] = await status(route(live, rest))
    expect(["items", "letters", "options", "latest"].every((rest) => result.live[rest] === 501), `${live.name} (${live.kind}) answers 501 on every grid route (${JSON.stringify(result.live)})`)
  } else {
    ctx.step("no library is served live on this server; the 501 answer is not exercised")
  }
  return result
}

export default async function librariesApi(ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)

  const report = { mode: ctx.account ? `account ${ctx.account.server}` : "demo", checks: [] }
  // Every check is recorded and the run fails only at the end, so a failing
  // run still reports every library.
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
  expect(listed.status === 200 && listed.body.known === true, `/api/libraries answers ${listed.status} with known=${listed.body?.known}`)
  expect(views.length > 0, `/UserViews lists ${views.length} libraries`)
  expect(
    sameList(report.libraries, report.userViews.map(({ id, name, kind }) => ({ id: id.toLowerCase(), name, kind }))),
    `ids, names, order and kinds equal /UserViews (${JSON.stringify(report.libraries.map((library) => `${library.name}:${library.kind}`))})`,
  )

  report.totals = []
  for (const library of libraries.filter((candidate) => TITLE_KIND[candidate.kind])) {
    const titleKind = TITLE_KIND[library.kind]
    const page = await ctx.api(`/api/libraries/${library.id}/items?limit=1`)
    const server = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${library.id}&Recursive=true&IncludeItemTypes=${titleKind}&Limit=0`)
    const entry = { id: library.id, name: library.name, kind: library.kind, titleKind, status: page.status, total: page.body?.total ?? null, jellyfinTotal: server.TotalRecordCount, firstItemKind: page.body?.items?.[0]?.kind ?? null, browse: library.browse }
    report.totals.push(entry)
    expect(page.status === 200 && entry.total === entry.jellyfinTotal, `${library.name}: total ${entry.total} equals Jellyfin's ${titleKind} count ${entry.jellyfinTotal}`)
    expect(entry.total === 0 || entry.firstItemKind === titleKind, `${library.name}: its grid lists ${titleKind} items (${entry.firstItemKind})`)
  }
  expect(report.totals.length > 0, `${report.totals.length} movies or TV shows libraries compared`)

  if (report.totals.length) report.contract = await contract(ctx, libraries, expect)

  ctx.writeEvidence("libraries-report.json", report)
  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
