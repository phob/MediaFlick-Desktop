// Library views without a media kind (sidebar search, Favorites, its A–Z
// rail, Home's Favorites shelf) list only movies and series, never seasons or
// episodes, while `kind=Episode` still lists episodes.
// Both modes: a sidebar search for a word from a season name that zero to six
// movies or series share, and one for a word from an episode name that one to
// six share, must each show exactly those titles (the empty state when there
// are none) and no season or episode card or API row. The kindless
// `favorite=false` reads and A–Z letters must match `kind=Movie,Series`, and
// Home rows hold only movies and series (Recently Added Shows: series and
// episodes).
// With --account only: favorites a movie and an episode that were not
// favorites, on their detail pages, then checks Favorites, its A–Z letters and
// Home's Favorites shelf list the movie and not the episode, and removes both
// favorites again the same way (with a journaled after-run fallback). The
// Favorites card check reads the rendered cards of a virtualized grid, so it
// assumes the account has few favorites.
//   just verify top-level-titles
//   just verify top-level-titles --account https://jellyfin.example.com
import { isFavorite } from "../scripts/favorites.mjs"

const TOP_LEVEL = new Set(["Movie", "Series"])
const HOME_ROW_KINDS = { recentlyAddedShows: new Set(["Series", "Episode"]) }
const CARD = { css: 'main a[aria-label^="Open details for"]' }
const MIN_SEARCH_HITS = { Season: 0, Episode: 1 }
const MAX_SEARCH_HITS = 6

const kindsOf = (items) => [...new Set(items.map((item) => item.kind))].sort()
const nested = (items) => items.filter((item) => !TOP_LEVEL.has(item.kind)).map((item) => `${item.kind}: ${item.name}`)
const idsOf = (items) => items.map((item) => item.id).sort()
const sameList = (left, right) => JSON.stringify(left) === JSON.stringify(right)
const query = (params) => `/api/items?${new URLSearchParams(params)}`
const cardFor = (id) => ({ css: `main a[aria-label^="Open details for"][href$="/item/${encodeURIComponent(id)}"]` })

async function items(ctx, params) {
  const response = await ctx.api(query(params))
  if (response.status !== 200) throw new Error(`${query(params)} answered ${response.status}`)
  return response.body
}

async function cardNames(ctx) {
  return ctx.app.evaluate(`[...document.querySelectorAll('main a[aria-label^="Open details for"]')].map((a) => a.getAttribute("aria-label").replace("Open details for ", ""))`)
}

async function search(ctx, term) {
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, term)
  await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1] ?? "").get("search") === term, `search=${term} in the URL`)
  await ctx.until(async () => {
    const text = await ctx.app.evaluate("document.querySelector('main')?.innerText ?? ''")
    return text.includes("Nothing to show") || (await ctx.count(CARD)) > 0
  }, `the results for ${term}`, 15000)
  await ctx.park()
}

async function sharedTerm(ctx, kind) {
  const names = (await items(ctx, { kind, limit: 500 })).items.map((item) => item.name.toLowerCase())
  const words = [...new Set(names.flatMap((name) => name.match(/\p{L}{4,}/gu) ?? []))].sort()
  for (const word of words) {
    const titles = await items(ctx, { search: word, kind: "Movie,Series", limit: MAX_SEARCH_HITS + 1 })
    if (titles.total < MIN_SEARCH_HITS[kind] || titles.total > MAX_SEARCH_HITS) continue
    const matches = await items(ctx, { search: word, kind, limit: 1 })
    if (matches.total > 0) return { term: word, kind, nestedMatches: matches.total, expected: titles.items }
  }
  return null
}

async function firstNotFavorite(ctx, candidates) {
  for (const item of candidates) if (!(await isFavorite(ctx.jellyfin, ctx.account.userId, item.id))) return item
  throw new Error(`no candidate is free of favorites on the server: ${JSON.stringify(candidates.map((item) => item.name))}`)
}

async function openMovie(ctx, movie) {
  await search(ctx, movie.name)
  await ctx.press(cardFor(movie.id), { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${encodeURIComponent(movie.id)}`, `${movie.name}'s page`)
}

async function openEpisode(ctx, episode, seasonName) {
  await search(ctx, episode.seriesName)
  await ctx.press(cardFor(episode.seriesId), { nth: 0 })
  await ctx.find({ role: "list", name: "Seasons" }, { timeout: 20000 })
  const link = { css: `main a[href$="/item/${encodeURIComponent(episode.id)}"]` }
  if (!(await ctx.exists(link))) {
    const position = await ctx.app.evaluate(`[...document.querySelectorAll('ul[aria-label="Seasons"] > li')].findIndex((li) => li.querySelector("button > span:last-child")?.textContent.trim() === ${JSON.stringify(seasonName)}) + 1`)
    if (position < 1) throw new Error(`no season button named ${seasonName}`)
    await ctx.press({ css: `ul[aria-label="Seasons"] > li:nth-child(${position}) > button` })
  }
  await ctx.press(link, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === `/item/${encodeURIComponent(episode.id)}`, `${episode.name}'s page`)
}

async function toggleFavorite(ctx, item, favorite) {
  const [from, to] = favorite ? ["Add to favorites", "Remove from favorites"] : ["Remove from favorites", "Add to favorites"]
  await ctx.press({ role: "button", name: from })
  await ctx.find({ role: "button", name: to })
  await ctx.until(async () => (await isFavorite(ctx.jellyfin, ctx.account.userId, item.id)) === favorite, `${item.kind} ${item.name} with IsFavorite=${favorite} on the server`)
}

async function cardIds(ctx) {
  return ctx.app.evaluate(`[...document.querySelectorAll('main a[aria-label^="Open details for"]')].map((a) => decodeURIComponent(a.getAttribute("href").split("/item/")[1]))`)
}

async function homeFavoritesShelf(ctx) {
  return ctx.app.evaluate(`(() => {
    const section = [...document.querySelectorAll("section[aria-labelledby]")].find((s) => s.querySelector("h2")?.textContent.trim() === "Favorites")
    if (!section) return null
    section.scrollIntoView({ block: "center" })
    return [...section.querySelectorAll('a[aria-label^="Open details for"]')].map((a) => decodeURIComponent(a.getAttribute("href").split("/item/")[1]))
  })()`)
}

async function provesFavorites(ctx, report, expect) {
  const movie = await firstNotFavorite(ctx, (await items(ctx, { kind: "Movie", favorite: "false", sort: "added", limit: 20 })).items.filter((item) => /\p{L}{3,}/u.test(item.name)))
  const episode = await firstNotFavorite(ctx, (await items(ctx, { kind: "Episode", favorite: "false", limit: 50 })).items.filter((item) => item.seriesId && item.seasonId && item.seriesName))
  const seasonName = (await ctx.api(`/api/item/${encodeURIComponent(episode.seasonId)}`)).body?.name
  const before = await items(ctx, { favorite: "true", limit: 500 })
  report.favorites = { movie: { id: movie.id, name: movie.name }, episode: { id: episode.id, name: episode.name, series: episode.seriesName, season: seasonName }, before: before.total }
  ctx.step(`favoriting movie ${movie.name} (${movie.id}) and episode ${episode.seriesName} ${seasonName} ${episode.name} (${episode.id}); ${before.total} top-level favorites before`)

  await ctx.unfavoriteAfterRun(movie.id)
  await openMovie(ctx, movie)
  await toggleFavorite(ctx, movie, true)
  await ctx.screenshot("favorite-movie")
  await ctx.unfavoriteAfterRun(episode.id)
  await openEpisode(ctx, episode, seasonName)
  await toggleFavorite(ctx, episode, true)
  await ctx.screenshot("favorite-episode")

  const favorites = await items(ctx, { favorite: "true", limit: 500 })
  const favoriteEpisodes = await items(ctx, { kind: "Episode", favorite: "true", limit: 500 })
  const letters = (await ctx.api("/api/items/letters?favorite=true")).body?.letters ?? null
  const topLevelLetters = (await ctx.api("/api/items/letters?kind=Movie,Series&favorite=true")).body?.letters ?? null

  await ctx.press({ role: "link", name: "Favorites" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?favorite=true", "the Favorites route")
  await ctx.until(async () => (await ctx.count(cardFor(movie.id))) > 0, `${movie.name} among the Favorites cards`)
  await ctx.park()
  const favoriteCards = await cardIds(ctx)
  await ctx.snapshot("favorites")
  await ctx.screenshot("favorites")

  await ctx.press({ role: "link", name: "Home" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  const shelf = await ctx.until(async () => {
    const ids = await homeFavoritesShelf(ctx)
    return ids?.includes(movie.id) ? ids : null
  }, `${movie.name} on Home's Favorites shelf`)
  await ctx.park()
  await ctx.screenshot("home-favorites")
  const homeRow = ((await ctx.api("/api/home")).body?.rows ?? []).find((row) => row.id === "myList") ?? null

  Object.assign(report.favorites, {
    listed: { total: favorites.total, kinds: kindsOf(favorites.items), names: favorites.items.map((item) => item.name) },
    favoriteEpisodes: idsOf(favoriteEpisodes.items),
    letters,
    topLevelLetters,
    favoriteCards,
    homeShelf: shelf,
    homeRow: homeRow && { kinds: kindsOf(homeRow.items), ids: homeRow.items.map((item) => item.id) },
  })

  await openMovie(ctx, movie)
  await toggleFavorite(ctx, movie, false)
  await openEpisode(ctx, episode, seasonName)
  await toggleFavorite(ctx, episode, false)
  report.favorites.after = { movie: await isFavorite(ctx.jellyfin, ctx.account.userId, movie.id), episode: await isFavorite(ctx.jellyfin, ctx.account.userId, episode.id), total: (await items(ctx, { favorite: "true", limit: 1 })).total }

  const listedIds = favorites.items.map((item) => item.id)
  expect(favoriteEpisodes.items.some((item) => item.id === episode.id), `kind=Episode&favorite=true lists the favorited episode ${episode.name}`)
  expect(listedIds.includes(movie.id) && !listedIds.includes(episode.id) && nested(favorites.items).length === 0, `favorite=true lists ${movie.name} and no season or episode (${JSON.stringify(report.favorites.listed.names)})`)
  expect(favoriteCards.includes(movie.id) && !favoriteCards.includes(episode.id), `the Favorites page shows ${movie.name} and not episode ${episode.name} (${JSON.stringify(favoriteCards)})`)
  expect(letters && Object.keys(letters).length > 0 && sameList(letters, topLevelLetters), `Favorites' A–Z rail matches its movies and series (${JSON.stringify(letters)} vs ${JSON.stringify(topLevelLetters)})`)
  expect(shelf.includes(movie.id) && !shelf.includes(episode.id), `Home's Favorites shelf shows ${movie.name} and not episode ${episode.name}`)
  expect(homeRow?.items.every((item) => TOP_LEVEL.has(item.kind)) && homeRow.items.some((item) => item.id === movie.id), `/api/home's myList row holds ${movie.name} and only movies and series (${JSON.stringify(report.favorites.homeRow?.kinds)})`)
  expect(report.favorites.after.movie === false && report.favorites.after.episode === false && report.favorites.after.total === before.total, `both favorites removed again on the server (${report.favorites.after.total} top-level favorites, ${before.total} before)`)
}

export default async function topLevelTitles(ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)

  const report = { mode: ctx.account ? `account ${ctx.account.server}` : "demo", searches: {}, reads: {}, checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  for (const kind of ["Season", "Episode"]) {
    const shared = await sharedTerm(ctx, kind)
    expect(shared, shared ? `search ${shared.term} matches ${shared.nestedMatches} ${kind} rows and ${shared.expected.length} movies or series` : `a ${kind} name shares a word with ${MIN_SEARCH_HITS[kind]} to ${MAX_SEARCH_HITS} movies or series`)
    if (!shared) continue
    const { term, expected } = shared
    await search(ctx, term)
    const cards = await cardNames(ctx)
    const cardItems = await cardIds(ctx)
    const emptyState = await ctx.app.evaluate("(document.querySelector('main')?.innerText ?? '').includes('Nothing to show')")
    const kindless = await items(ctx, { search: term, limit: 60 })
    report.searches[term] = {
      nestedKind: kind,
      nestedMatches: shared.nestedMatches,
      expected: expected.map((item) => `${item.kind}: ${item.name}`),
      cards,
      emptyState,
      apiKinds: kindsOf(kindless.items),
      nested: nested(kindless.items),
    }
    ctx.step(`search ${term} (${shared.nestedMatches} ${kind} matches): cards ${JSON.stringify(cards)}`)
    await ctx.snapshot(`search-${term}`)
    await ctx.screenshot(`search-${term}`)
    expect(sameList(idsOf(kindless.items), idsOf(expected)), `search ${term} lists exactly its ${expected.length} movies or series and no ${kind} (got ${JSON.stringify(report.searches[term].nested)} besides)`)
    expect(sameList([...cardItems].sort(), idsOf(expected)), `search ${term} shows one card per movie or series and no ${kind} card (${JSON.stringify(cards)})`)
    if (!expected.length) expect(emptyState, `search ${term} shows the empty state`)
  }

  const notFavorite = await items(ctx, { favorite: "false", limit: 500 })
  const topLevel = await items(ctx, { kind: "Movie,Series", favorite: "false", limit: 1 })
  const nestedRows = await items(ctx, { kind: "Season,Episode", favorite: "false", limit: 1 })
  const episodes = await items(ctx, { kind: "Episode", limit: 200 })
  const letters = await ctx.api("/api/items/letters?favorite=false")
  const topLevelLetters = await ctx.api("/api/items/letters?kind=Movie,Series&favorite=false")
  report.reads = {
    notFavorite: { total: notFavorite.total, kinds: kindsOf(notFavorite.items), nested: nested(notFavorite.items).slice(0, 10), topLevelTotal: topLevel.total, nestedRowsTotal: nestedRows.total },
    explicitEpisodes: { total: episodes.total, kinds: kindsOf(episodes.items) },
    letters: { status: letters.status, kindless: letters.body?.letters ?? null, movieAndSeries: topLevelLetters.body?.letters ?? null },
    homeRows: ((await ctx.api("/api/home")).body?.rows ?? []).map((row) => ({ id: row.id, kinds: kindsOf(row.items) })),
  }

  const { notFavorite: read } = report.reads
  expect(read.topLevelTotal > 0 && read.nestedRowsTotal > 0, `the cache holds ${read.topLevelTotal} movies and series and ${read.nestedRowsTotal} seasons and episodes that are not favorites`)
  expect(read.total === read.topLevelTotal && read.nested.length === 0, `a kindless favorite=false view counts only movies and series (${read.total} vs ${read.topLevelTotal}; nested ${JSON.stringify(read.nested)})`)
  const { kindless, movieAndSeries } = report.reads.letters
  expect(letters.status === 200 && Object.keys(kindless ?? {}).length > 0 && sameList(kindless, movieAndSeries), `a kindless view's A–Z rail matches Movies and Series (${JSON.stringify(kindless)} vs ${JSON.stringify(movieAndSeries)})`)
  expect(episodes.total > 0 && sameList(report.reads.explicitEpisodes.kinds, ["Episode"]), `kind=Episode still lists episodes (${episodes.total})`)
  expect(report.reads.homeRows.every((row) => row.kinds.every((kind) => (HOME_ROW_KINDS[row.id] ?? TOP_LEVEL).has(kind))), `Home rows hold only movies and series, Recently Added Shows only series and episodes (${JSON.stringify(report.reads.homeRows)})`)

  if (ctx.account) await provesFavorites(ctx, report, expect)
  else ctx.step("no --account: Favorites and Home's Favorites shelf are not exercised, because the shared demo account must not be written")

  ctx.writeEvidence("top-level-titles.json", report)
  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
