// Studio, Network and Tag filters on the library grids, and the studio and tag
// links on a title page. Read-only on the server; writes only the profile.
//   just verify library-studio-tag --account https://jellyfin.example.com
//
// The oracle is Jellyfin's own data: every movie and series of the account's
// first movies and TV shows libraries with their Studios and Tags, matched by
// exact name as the app's filters match. Jellyfin's `StudioIds` and `Tags`
// query counts are recorded next to it.
//
// 1. Turns on Remember library filters (Settings > Viewing, Save).
// 2. Movies library: Filters > Studio, a search in the picker, a pick; the
//    chip, the URL, the grid count and the grid's ids match the oracle.
// 3. The same for Filters > Tag.
// 4. Home, then the library again: the remembered tag filter comes back.
// 5. TV shows library: the menu offers Network, not Studio; a network pick
//    matches the oracle.
// 6. The combined /library view (every movie and series, reached by
//    clearing a sidebar search): Studio and Tag picks list exactly the titles
//    of every library that carry them.
// 7. A movie's page: its studio link opens the movies library filtered to that
//    studio, and one of its tag links opens it filtered to that tag.
const LIBRARY_ROUTE = /^\/library\/[0-9a-f]+/
const itemCount = (ctx) =>
  ctx.app.evaluate(`(() => { const m = document.body.innerText.match(/^([\\d,]+) items?$/m); return m ? Number(m[1].replace(/,/g, "")) : null })()`)
const MIN = 2
const MAX = 60

function exactly(items, field, value) {
  return items.filter((item) => names(item, field).includes(value)).map((item) => item.Id).sort()
}

function names(item, field) {
  return field === "Studios" ? (item.Studios ?? []).map((studio) => studio.Name) : item[field] ?? []
}

export default async function libraryStudioTag(ctx) {
  if (!ctx.account) throw new Error("run with --account SERVER: the demo server's titles carry no tags")
  const report = { server: ctx.account.server, checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const sameIds = (a, b) => a.length === b.length && a.every((id, index) => id === b[index])

  await ctx.signIn({})
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  const libraries = (await ctx.api("/api/libraries")).body.libraries
  const movies = libraries.find((library) => library.kind === "movies")
  const shows = libraries.find((library) => library.kind === "shows")
  if (!movies || !shows) throw new Error("the account needs a movies and a TV shows library")
  report.libraries = { movies: movies.name, shows: shows.name }
  expect(movies.browse.filters.includes("studio") && movies.browse.filters.includes("tag"), "the movies library offers studio and tag filters")
  expect(shows.browse.filters.includes("studio") && shows.browse.filters.includes("tag"), "the TV shows library offers studio and tag filters")

  const userId = ctx.account.userId
  const titles = async (library, type) =>
    (await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${library.id}&Recursive=true&IncludeItemTypes=${type}&Fields=Studios,Tags,Genres`)).Items
  const movieItems = await titles(movies, "Movie")
  const seriesItems = await titles(shows, "Series")
  const nameCount = new Map()
  for (const item of [...movieItems, ...seriesItems]) nameCount.set(item.Name, (nameCount.get(item.Name) ?? 0) + 1)
  const fits = (ids) => ids.length >= MIN && ids.length <= MAX

  let pick = null
  for (const movie of movieItems) {
    if (nameCount.get(movie.Name) !== 1 || !/^[\w .,:'&-]+$/.test(movie.Name)) continue
    const studio = names(movie, "Studios").find((name) => fits(exactly(movieItems, "Studios", name)))
    const genres = (movie.Genres ?? []).map((genre) => genre.toLowerCase())
    const tag = names(movie, "Tags").find((name) =>
      !name.includes("|") && !genres.includes(name.toLowerCase()) && name !== studio && name !== movie.Name
      && fits(exactly(movieItems, "Tags", name)))
    if (studio && tag) {
      pick = { movie, studio, tag }
      break
    }
  }
  if (!pick) throw new Error(`no movie with a studio and a tag shared by ${MIN} to ${MAX} movies`)
  const network = [...new Set(seriesItems.flatMap((series) => names(series, "Studios")))]
    .find((name) => fits(exactly(seriesItems, "Studios", name)))
  if (!network) throw new Error(`no network shared by ${MIN} to ${MAX} series`)

  const studioId = pick.movie.Studios.find((studio) => studio.Name === pick.studio).Id
  const networkId = seriesItems.flatMap((series) => series.Studios).find((studio) => studio.Name === network).Id
  const jellyfinCount = async (query) =>
    (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&Limit=0&${query}`)).TotalRecordCount
  const oracle = {
    movie: { id: pick.movie.Id, name: pick.movie.Name },
    studio: { name: pick.studio, ids: exactly(movieItems, "Studios", pick.studio), jellyfinStudioIdsCount: await jellyfinCount(`ParentId=${movies.id}&IncludeItemTypes=Movie&StudioIds=${studioId}`) },
    tag: { name: pick.tag, ids: exactly(movieItems, "Tags", pick.tag), jellyfinTagsCount: await jellyfinCount(`ParentId=${movies.id}&IncludeItemTypes=Movie&Tags=${encodeURIComponent(pick.tag)}`) },
    network: { name: network, ids: exactly(seriesItems, "Studios", network), jellyfinStudioIdsCount: await jellyfinCount(`ParentId=${shows.id}&IncludeItemTypes=Series&StudioIds=${networkId}`) },
  }
  report.oracle = oracle
  ctx.step(`oracle: movie ${oracle.movie.name}, studio ${oracle.studio.name} (${oracle.studio.ids.length}), tag ${oracle.tag.name} (${oracle.tag.ids.length}), network ${network} (${oracle.network.ids.length})`)

  const everyTitle = (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Movie,Series&Fields=Studios,Tags`)).Items
  oracle.everywhere = {
    studio: { name: pick.studio, ids: exactly(everyTitle, "Studios", pick.studio) },
    tag: { name: pick.tag, ids: exactly(everyTitle, "Tags", pick.tag) },
  }

  const appIds = async (library, param, value) => {
    const answer = await ctx.api(library
      ? `/api/libraries/${library.id}/items?${param}=${encodeURIComponent(value)}&limit=200`
      : `/api/items?${param}=${encodeURIComponent(value)}&limit=200`)
    return { total: answer.body.total, ids: answer.body.items.map((item) => item.id).sort() }
  }
  const expectGrid = async (label, library, param, expected) => {
    await ctx.until(async () => (await itemCount(ctx)) === expected.ids.length, `${label}: the grid to count ${expected.ids.length}`, 30000)
    const app = await appIds(library, param, expected.name)
    report[label] = { url: await ctx.pathname(), gridCount: await itemCount(ctx), apiTotal: app.total, idsMatch: sameIds(app.ids, expected.ids) }
    expect(app.total === expected.ids.length && sameIds(app.ids, expected.ids), `${label}: the ${library ? "library's" : "combined view's"} ${param} filter lists exactly the ${expected.ids.length} titles Jellyfin names ${expected.name}`)
  }
  const openLibrary = async (library) => {
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.press({ role: "link", name: library.name })
    await ctx.until(async () => (await ctx.pathname()).startsWith(`/library/${library.id}`), `the ${library.name} library`)
    await ctx.park()
  }
  const filterBy = async (label, value) => {
    // The trigger's name carries the active count, which a chip removal changes.
    await ctx.press({ css: 'button[aria-label^="Filters"]' })
    const menu = await ctx.snapshot(`${label}-menu`)
    const items = [...menu.matchAll(/menuitem '([^']*)'/g)].map((match) => match[1])
    const sub = items.find((name) => name.startsWith(`${label} `))
    if (!sub) throw new Error(`the Filters menu has no ${label} entry: ${items.join(", ")}`)
    await ctx.press({ role: "menuitem", name: sub })
    const noun = label.toLowerCase()
    await ctx.fill({ role: "combobox", name: `Search ${noun}s` }, value)
    await ctx.press({ role: "option", name: value })
    await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get(label === "Network" ? "studio" : noun) === value, `${label} ${value} in the URL`)
    await ctx.until(async () => (await ctx.count({ role: "menu" })) === 0, "the menu to close after a pick", 3000)
    await ctx.find({ role: "button", name: `Remove ${label}: ${value} filter` })
    return items
  }
  const openTitle = async (name) => {
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.fill({ role: "textbox", name: "Search the library" }, name)
    await ctx.until(async () => (await ctx.pathname()) === `/library?search=${encodeURIComponent(name)}`, "the search route")
    await ctx.press({ role: "link", name: `Open details for ${name}` })
    await ctx.until(async () => (await ctx.pathname()).startsWith(`/item/${pick.movie.Id}`), `${name}'s page`)
    await ctx.park()
  }

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Viewing", within: { role: "navigation", name: "Settings navigation" } })
  await ctx.press({ role: "switch", name: "Remember library filters" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => (await ctx.api("/api/settings/viewing")).body?.rememberFilters === true, "Remember library filters to be saved")

  await openLibrary(movies)
  const movieMenu = await filterBy("Studio", oracle.studio.name)
  report.movieMenu = movieMenu
  await ctx.park()
  await expectGrid("movies-studio", movies, "studio", oracle.studio)
  await ctx.screenshot("01-movies-studio")
  await ctx.press({ role: "button", name: `Remove Studio: ${oracle.studio.name} filter` })
  await ctx.until(async () => !(await ctx.pathname()).includes("studio="), "the studio filter to go")

  await filterBy("Tag", oracle.tag.name)
  await ctx.park()
  await expectGrid("movies-tag", movies, "tag", oracle.tag)
  await ctx.screenshot("02-movies-tag")

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Home" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await openLibrary(movies)
  await ctx.find({ role: "button", name: `Remove Tag: ${oracle.tag.name} filter` })
  await expectGrid("movies-tag-remembered", movies, "tag", oracle.tag)
  const browsing = (await ctx.api("/api/settings/browsing")).body
  report.rememberedRoute = browsing?.[`library:${movies.id}`] ?? null
  expect(new URLSearchParams(report.rememberedRoute?.split("?")[1] ?? "").get("tag") === oracle.tag.name, `the remembered route for ${movies.name} carries the tag`)
  await ctx.screenshot("03-movies-tag-remembered")
  await ctx.press({ role: "button", name: "Clear all" })
  await ctx.until(async () => !(await ctx.exists({ role: "group", name: "Active filters" })), "the chips to go")

  await openLibrary(shows)
  const showMenu = await filterBy("Network", network)
  report.showMenu = showMenu
  expect(!showMenu.some((name) => name.startsWith("Studio ")), "the TV shows library labels the studio filter Network")
  expect(movieMenu.some((name) => name.startsWith("Studio ")) && !movieMenu.some((name) => name.startsWith("Network ")), "the movies library labels it Studio")
  await ctx.park()
  await expectGrid("shows-network", shows, "studio", oracle.network)
  await ctx.screenshot("04-shows-network")
  await ctx.press({ role: "button", name: "Clear all" })
  await ctx.until(async () => !(await ctx.exists({ role: "group", name: "Active filters" })), "the chips to go")

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, oracle.movie.name)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?search="), "the search route")
  await ctx.fill({ role: "textbox", name: "Search the library" }, "")
  await ctx.until(async () => (await ctx.pathname()) === "/library", "the combined view")
  await ctx.find({ role: "heading", name: "Library" })
  await ctx.park()
  await filterBy("Studio", oracle.studio.name)
  await ctx.park()
  await expectGrid("everywhere-studio", null, "studio", oracle.everywhere.studio)
  await ctx.screenshot("05-everywhere-studio")
  await ctx.press({ role: "button", name: `Remove Studio: ${oracle.studio.name} filter` })
  await ctx.until(async () => !(await ctx.pathname()).includes("studio="), "the studio filter to go")
  await filterBy("Tag", oracle.tag.name)
  await ctx.park()
  await expectGrid("everywhere-tag", null, "tag", oracle.everywhere.tag)
  await ctx.screenshot("06-everywhere-tag")
  await ctx.press({ role: "button", name: "Clear all" })
  await ctx.until(async () => !(await ctx.exists({ role: "group", name: "Active filters" })), "the chips to go")

  await openTitle(oracle.movie.name)
  await ctx.find({ role: "link", name: oracle.studio.name })
  await ctx.find({ role: "link", name: oracle.tag.name })
  await ctx.screenshot("07-detail")
  await ctx.press({ role: "link", name: oracle.studio.name })
  await ctx.until(async () => (await ctx.pathname()) === `/library/${movies.id}?${new URLSearchParams({ studio: oracle.studio.name })}`, "the studio link to open the movies library filtered to it")
  await ctx.find({ role: "button", name: `Remove Studio: ${oracle.studio.name} filter` })
  await ctx.park()
  await expectGrid("detail-studio", movies, "studio", oracle.studio)
  await ctx.screenshot("08-detail-studio-link")

  await openTitle(oracle.movie.name)
  await ctx.press({ role: "link", name: oracle.tag.name })
  await ctx.until(async () => (await ctx.pathname()) === `/library/${movies.id}?${new URLSearchParams({ tag: oracle.tag.name })}`, "the tag link to open the movies library filtered to it")
  await ctx.find({ role: "button", name: `Remove Tag: ${oracle.tag.name} filter` })
  await ctx.park()
  await expectGrid("detail-tag", movies, "tag", oracle.tag)
  await ctx.screenshot("09-detail-tag-link")

  await ctx.writeEvidence("library-studio-tag.json", report)
  const failed = report.checks.filter((check) => !check.ok)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.map((check) => check.message).join("; ")}`)
}
