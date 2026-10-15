// Generic details pages. Nothing links to an album or a box set's own page
// until the library browse change, so the drive opens them by URL with no
// navigation state, as a reload or deep link does.
// Both modes: while the first sync still runs, the newest movie, which that
// walk reaches last, is asked for before the cache holds it and comes back as
// a title that the app logs as served through the cache-through path and
// caches on the way (required with --account; the demo walk can finish
// first). One of the newest series the cache lacks opens its page with its
// seasons and episodes read from the cache, never live from the server. A
// folder page asks the server only for its grid's pages, never for an unpaged
// list. The Music library's first album with artwork and tracks shows no
// title controls (Play, favorites, watched, More info, quality, ratings,
// collection chip, Letterboxd, Media), goes back to Home, contains its cover
// in the hero, contains near-square track art in the cards and fills wide art,
// and its grid holds exactly the tracks Jellyfin lists, as many as the album's
// ChildCount; a track opens a generic page with no contents grid. The first
// MusicArtist folder lists exactly its albums. The Movies library's own folder
// page lists exactly its direct children, paged in as the page scrolls, and
// its children route clamps limit=100000 to 200. The top-rated movie, opened
// from a sidebar search, keeps its title page: Play or Resume, quality,
// watched and favorites, More info, its community rating and its library. An
// episode reports its series' library. A series page shows its season rail,
// the selected season's episodes and a Play button naming Next up, and a
// season's own page opens the series on that season. Every item route rejects
// a bad id, and the children route a negative offset.
// With --account only: a movie in one of the account's franchises shows its
// collection chip; the Collections library's largest box set lists exactly
// its titles; an unwatched movie in its first row, marked watched and then
// unwatched in Jellyfin itself, follows on its open card each time (restored
// after the run); a movie card that is not a favorite is favorited from its
// preview and shows it on the next hover without leaving the page, then is
// unfavorited the same way (journaled after-run fallback); and a movie card
// opens the full movie page.
//   just verify generic-detail
//   just verify generic-detail --account https://jellyfin.example.com
const DEMO = "https://demo.jellyfin.org/stable"
const HERO = '[data-testid="detail-hero-layout"]'
const GRID_CARD_LINK = '[data-item-grid-canvas] article a[aria-label^="Open details for"]'
const TITLE_CONTROLS = /^(Play|Resume|Play S\d+E\d+|Resume S\d+E\d+|Add to favorites|Remove from favorites|Mark watched|Mark unwatched|More info|Streaming quality)$/
// ui/src/lib/artwork.ts: near-square art is contained in a card, wider art fills it.
const SQUARE_ART = { min: 0.8, max: 1.3 }
const LIVE_PAGE_MAX = 200

const sameList = (left, right) => JSON.stringify(left) === JSON.stringify(right)
const idFromHref = (href) => decodeURIComponent(href.replace(/^.*\/item\//, ""))

async function openByUrl(ctx, path) {
  await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(path)}); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(async () => (await ctx.pathname()) === path, `the ${path} route`)
}

async function openFromSearch(ctx, name) {
  await ctx.park()
  if ((await ctx.pathname()) !== "/") await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, name)
  await ctx.until(async () => (await ctx.pathname()) === `/library?search=${encodeURIComponent(name)}`, "the search route")
  await ctx.press({ role: "link", name: `Open details for ${name}` }, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `the ${name} page`)
}

async function surfaces(ctx) {
  return ctx.app.evaluate(`(() => {
    const hero = document.querySelector(${JSON.stringify(HERO)})
    if (!hero) return null
    const poster = hero.querySelector("img.media-artwork-image")
    return {
      controls: [...hero.querySelectorAll("button, a, [role=combobox]")]
        .map((element) => (element.getAttribute("aria-label") ?? element.textContent).trim())
        .filter(Boolean),
      back: [...hero.querySelectorAll("a")].map((link) => link.textContent.trim()).find((text) => text.startsWith("Back")) ?? null,
      communityRating: Boolean(hero.querySelector('[aria-label^="Jellyfin community rating"]')),
      ratings: Boolean(hero.querySelector('[aria-label^="Ratings for"]')),
      collectionChip: Boolean(hero.querySelector('a[href^="/collections/franchises/"]')),
      genreLinks: hero.querySelectorAll('a[href^="/library?"]').length,
      letterboxd: Boolean(document.getElementById("letterboxd-reviews-heading")),
      media: [...document.querySelectorAll("h2")].some((heading) => heading.textContent.trim() === "Media"),
      contents: [...document.querySelectorAll("h2")].some((heading) => heading.textContent.trim() === "Contents"),
      posterFit: poster ? getComputedStyle(poster).objectFit : null,
      posterBox: poster ? Math.round((poster.clientWidth / poster.clientHeight) * 100) / 100 : null,
    }
  })()`)
}

async function gridCards(ctx) {
  return ctx.app.evaluate(`[...document.querySelectorAll(${JSON.stringify(GRID_CARD_LINK)})].map((link) => {
    const image = link.querySelector("img")
    return { href: link.getAttribute("href"), name: link.getAttribute("aria-label").slice("Open details for ".length), fit: image ? getComputedStyle(image).objectFit : null }
  })`)
}

async function wheelDown(ctx) {
  const [width, height] = await ctx.app.evaluate("[innerWidth, innerHeight]")
  await ctx.app.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: width / 2, y: height / 2 })
  await ctx.app.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: width / 2, y: height / 2, deltaX: 0, deltaY: Math.round(height * 0.7) })
}

async function collectGrid(ctx, expected) {
  const seen = new Map()
  for (let turn = 0; turn < 400; turn += 1) {
    await ctx.until(`!document.querySelector("[data-item-grid-canvas] [data-slot=skeleton]")`, "the visible grid pages to load", 30000)
    for (const card of await gridCards(ctx)) seen.set(idFromHref(card.href), card)
    if (seen.size >= expected) break
    const before = await ctx.app.evaluate(`document.querySelector(".content-viewport").scrollTop`)
    await wheelDown(ctx)
    const moved = await ctx.until(async () => (await ctx.app.evaluate(`document.querySelector(".content-viewport").scrollTop`)) !== before, "the page to scroll", 3000).then(() => true, () => false)
    if (!moved) {
      for (const card of await gridCards(ctx)) seen.set(idFromHref(card.href), card)
      break
    }
  }
  await ctx.app.evaluate(`document.querySelector(".content-viewport").scrollTo({ top: 0, behavior: "instant" })`)
  return seen
}

async function jellyfinChildren(ctx, userId, parentId) {
  const items = []
  let total = null
  for (;;) {
    const page = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${parentId}&SortBy=ParentIndexNumber,IndexNumber,SortName&Fields=PrimaryImageAspectRatio&EnableUserData=false&StartIndex=${items.length}&Limit=${LIVE_PAGE_MAX}`)
    total ??= page.TotalRecordCount
    items.push(...page.Items.map((item) => ({ id: item.Id, name: item.Name, type: item.Type, aspect: item.PrimaryImageAspectRatio ?? null })))
    if (page.Items.length < LIVE_PAGE_MAX) break
  }
  return { total, items }
}

function noTitleSurfaces(found, label, expect) {
  const controls = found.controls.filter((name) => TITLE_CONTROLS.test(name))
  expect(controls.length === 0, `${label}: the hero has no Play, favorites, watched, More info or quality control (${JSON.stringify(controls)})`)
  expect(!found.communityRating && !found.ratings, `${label}: no community rating or ratings readout`)
  expect(!found.collectionChip, `${label}: no collection chip`)
  expect(!found.letterboxd && !found.media, `${label}: no Letterboxd or Media section`)
  expect(found.genreLinks === 0, `${label}: genres are plain chips, not links (${found.genreLinks} links)`)
}

async function genericFolder(ctx, expect, { label, id, name, userId, childCount }) {
  const detail = await ctx.api(`/api/item/${id}`)
  expect(detail.status === 200 && detail.body?.layout === "generic" && detail.body.isFolder === true, `${label}: /api/item answers the generic layout for a folder (${detail.status}, ${detail.body?.layout}, isFolder=${detail.body?.isFolder})`)
  await openByUrl(ctx, `/item/${id}`)
  await ctx.find({ role: "heading", name }, { timeout: 30000 })
  await ctx.find({ role: "heading", name: "Contents" })
  const server = await jellyfinChildren(ctx, userId, id)
  const seen = await collectGrid(ctx, server.total)
  const found = await surfaces(ctx)
  const firstPage = await ctx.api(`/api/item/${id}/children?offset=0&limit=60`)
  const unpaged = servedChildrenLive(await ctx.appLog(), id).filter((line) => line.includes("paged=false"))
  const shownIds = [...seen.keys()]
  const result = {
    id,
    name,
    childCount,
    jellyfinTotal: server.total,
    apiTotal: firstPage.body?.total ?? null,
    cards: seen.size,
    cardNames: [...seen.values()].map((card) => card.name),
    missing: server.items.filter((item) => !seen.has(item.id)).map((item) => item.name),
    extra: shownIds.filter((cardId) => !server.items.some((item) => item.id === cardId)),
    surfaces: found,
    server,
    seen,
  }
  noTitleSurfaces(found, label, expect)
  expect(found.back === "Back to home", `${label}: without navigation state the back link goes Home (${found.back})`)
  expect(result.apiTotal === server.total, `${label}: /api/item/{id}/children total ${result.apiTotal} equals Jellyfin's ${server.total}`)
  expect(result.missing.length === 0 && result.extra.length === 0 && seen.size === server.total, `${label}: the grid shows exactly Jellyfin's ${server.total} children (${seen.size} cards, missing ${JSON.stringify(result.missing)}, extra ${JSON.stringify(result.extra)})`)
  expect(sameList(shownIds, server.items.map((item) => item.id)), `${label}: the cards follow Jellyfin's disc, track and name order`)
  expect(unpaged.length === 0, `${label}: the page asks the server only for the grid's pages of children, never for an unpaged list (${unpaged.length} unpaged)`)
  return result
}

async function previewShowsFavorite(ctx, card) {
  await ctx.park()
  await ctx.until(`!document.querySelector(".preview-panel")`, "the card preview to close", 10000)
  await ctx.hover(card)
  await ctx.until(async () => (await ctx.exists({ role: "button", name: "Add to favorites" })) || (await ctx.exists({ role: "button", name: "Remove from favorites" })), "the card preview's favorites toggle", 10000)
  return ctx.exists({ role: "button", name: "Remove from favorites" })
}

async function cardShows(ctx, card, favorite) {
  return ctx.until(async () => (await previewShowsFavorite(ctx, card)) === favorite, `the card preview to show ${favorite ? "a favorite" : "no favorite"}`, 20000).then(() => true, () => false)
}

async function serverFavorite(ctx, userId, id) {
  return (await ctx.jellyfin(`/Items/${id}?userId=${userId}`)).UserData?.IsFavorite === true
}

async function favoriteFromCard(ctx, expect, { label, setId, userId }) {
  const firstRow = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${setId}&IncludeItemTypes=Movie&SortBy=ParentIndexNumber,IndexNumber,SortName&EnableUserData=true&Limit=6`)
  const movie = firstRow.Items.find((item) => !item.UserData?.IsFavorite)
  expect(Boolean(movie), `${label}: a movie card in the first row that is not a favorite (${movie?.Name})`)
  if (!movie) return null
  const card = { css: `${GRID_CARD_LINK}[href$="/item/${encodeURIComponent(movie.Id)}"]` }
  const result = { id: movie.Id, name: movie.Name }
  await ctx.unfavoriteAfterRun(movie.Id)
  result.startsUnfavorite = (await previewShowsFavorite(ctx, card)) === false
  expect(result.startsUnfavorite, `${label}: the ${movie.Name} card starts as not a favorite`)
  await ctx.press({ role: "button", name: "Add to favorites" })
  result.serverAfterAdd = await ctx.until(() => serverFavorite(ctx, userId, movie.Id), "Jellyfin to record the favorite", 15000).then(() => true, () => false)
  result.apiAfterAdd = (await ctx.api(`/api/item/${setId}/children?offset=0&limit=60`)).body?.items?.find((item) => item.id === movie.Id)?.favorite ?? null
  result.cardAfterAdd = await cardShows(ctx, card, true)
  await ctx.screenshot("05b-box-set-card-favorited")
  expect(result.serverAfterAdd && result.apiAfterAdd === true, `${label}: Jellyfin and the children route report ${movie.Name} as a favorite (server ${result.serverAfterAdd}, api ${result.apiAfterAdd})`)
  expect(result.cardAfterAdd, `${label}: the ${movie.Name} card shows the favorite without leaving the page`)
  if (result.cardAfterAdd) {
    await ctx.press({ role: "button", name: "Remove from favorites" })
    result.serverCleared = await ctx.until(async () => !(await serverFavorite(ctx, userId, movie.Id)), "Jellyfin to drop the favorite", 15000).then(() => true, () => false)
    result.cardAfterRemove = await cardShows(ctx, card, false)
    expect(result.serverCleared && result.cardAfterRemove, `${label}: unfavoriting from the card clears it on the server and on the card (server cleared ${result.serverCleared}, card ${result.cardAfterRemove})`)
  }
  await ctx.park()
  return result
}

// The change is made in Jellyfin, not in the app: the app only hears of it
// through the server's push, whose batch names the movie and its library
// folder, never the box set the open grid shows.
async function remoteWatchedFollows(ctx, expect, { label, setId, userId }) {
  const firstRow = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${setId}&IncludeItemTypes=Movie&SortBy=ParentIndexNumber,IndexNumber,SortName&EnableUserData=true&Limit=6`)
  const movie = firstRow.Items.find((item) => !item.UserData?.Played && !item.UserData?.PlaybackPositionTicks)
  expect(Boolean(movie), `${label}: a movie card in the first row that is unwatched with no resume point (${movie?.Name})`)
  if (!movie) return null
  const card = `${GRID_CARD_LINK}[href$="/item/${encodeURIComponent(movie.Id)}"]`
  const cardWatched = () => ctx.app.evaluate(`(() => { const link = document.querySelector(${JSON.stringify(card)}); return link ? Boolean(link.querySelector('[title="Watched"]')) : null })()`)
  const cachedPlayed = async () => (await ctx.api(`/api/item/${movie.Id}`)).body?.played
  const path = await ctx.pathname()
  const result = { id: movie.Id, name: movie.Name, startsUnwatched: (await cardWatched()) === false }
  ctx.restoreItemAfterRun(movie.Id)
  for (const played of [true, false]) {
    await ctx.setPlayedOnServer(movie.Id, played)
    const cached = await ctx.until(async () => (await cachedPlayed()) === played, `the app to apply the pushed ${played ? "watched" : "unwatched"} state`, 20000).then(() => true, () => false)
    const shown = await ctx.until(async () => (await cardWatched()) === played, `the ${movie.Name} card to show it ${played ? "watched" : "unwatched"}`, 10000).then(() => true, () => false)
    result[played ? "watched" : "unwatched"] = { cached, shown, stayed: (await ctx.pathname()) === path }
    if (played) await ctx.screenshot("05c-box-set-card-remote-watched")
  }
  expect(result.startsUnwatched, `${label}: the ${movie.Name} card starts unwatched`)
  for (const state of ["watched", "unwatched"]) {
    const { cached, shown, stayed } = result[state]
    expect(cached && shown && stayed, `${label}: ${movie.Name} marked ${state} in Jellyfin reaches the app's cache and the open card without leaving the page (cache ${cached}, card ${shown}, same page ${stayed})`)
  }
  return result
}

async function newestCachedIds(ctx, kind) {
  return new Set(((await ctx.api(`/api/items?kind=${kind}&sort=added&order=desc&limit=500`)).body?.items ?? []).map((item) => item.id))
}

function servedChildrenLive(log, id) {
  return log.split("\n").filter((line) => line.includes("fetched a container's children from the server") && line.includes(`item_id="${id}"`))
}

async function uncachedTitle(ctx, expect, userId) {
  const newest = (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Movie&SortBy=DateCreated&SortOrder=Descending&Limit=1`)).Items[0]
  if (!newest) return null
  const status = (await ctx.api("/api/status")).body
  const result = { id: newest.Id, name: newest.Name, bootstrapBefore: status.bootstrap, cachedBefore: (await newestCachedIds(ctx, "Movie")).has(newest.Id) }
  if (result.cachedBefore) {
    const reason = `the first sync had already cached ${newest.Name}, the newest movie, before the drive could ask for it`
    if (ctx.account) expect(false, `uncached title: ${reason}`)
    else ctx.step(`uncached title: ${reason}; the demo walk is too short to force the uncached path`)
    return result
  }
  const detail = await ctx.api(`/api/item/${newest.Id}`)
  result.layout = detail.body?.layout ?? null
  result.kind = detail.body?.kind ?? null
  result.libraryId = detail.body?.libraryId ?? null
  result.logged = (await ctx.appLog()).split("\n").some((line) => line.includes("served a title the cache did not hold") && line.includes(newest.Id))
  result.cachedAfter = (await newestCachedIds(ctx, "Movie")).has(newest.Id)
  expect(detail.status === 200 && result.layout === "title" && result.kind === "Movie", `uncached title ${newest.Name}: /api/item answers the title layout of a Movie (${detail.status}, ${result.layout}, ${result.kind})`)
  expect(result.logged && result.cachedAfter, `uncached title ${newest.Name}: the app served it through the cache-through path and cached it on the way (logged ${result.logged}, cached ${result.cachedAfter})`)
  return result
}

// Asked for before the series is cached, its seasons and their episodes would
// come live from the server, uncached and never reconciled.
async function uncachedSeries(ctx, expect, userId) {
  const newest = (await ctx.jellyfin(`/Items?userId=${userId}&Recursive=true&IncludeItemTypes=Series&SortBy=DateCreated&SortOrder=Descending&Fields=ChildCount&Limit=10`)).Items
  const cached = await newestCachedIds(ctx, "Series")
  const series = newest.find((item) => item.ChildCount > 0 && !cached.has(item.Id))
  if (!series) {
    const reason = "the first sync had already cached the ten newest series before the drive could ask for one"
    if (ctx.account) expect(false, `uncached series: ${reason}`)
    else ctx.step(`uncached series: ${reason}; the demo walk is too short to force the uncached path`)
    return null
  }
  const seasons = (await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${series.Id}&SortBy=ParentIndexNumber,IndexNumber,SortName`)).Items.filter((item) => item.Type === "Season")
  await openByUrl(ctx, `/item/${series.Id}`)
  await ctx.find({ role: "heading", name: series.Name }, { timeout: 30000 })
  await ctx.find({ role: "list", name: "Seasons" }, { timeout: 30000 })
  const rail = await ctx.until(async () => { const found = await seasonRail(ctx); return found.some((season) => season.pressed) ? found : null }, "a selected season", 15000)
  const selected = seasons.find((season) => season.Name === rail.find((entry) => entry.pressed)?.name)
  const episodes = selected ? (await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${selected.Id}&IncludeItemTypes=Episode&Limit=0`)).TotalRecordCount : 0
  const shown = await ctx.until(async () => { const links = await episodeLinks(ctx); return links.length === episodes ? links : null }, `the ${selected?.Name} episodes`, 15000).catch(() => null)
  const log = await ctx.appLog()
  const result = {
    id: series.Id,
    name: series.Name,
    servedUncached: log.split("\n").some((line) => line.includes("served a title the cache did not hold") && line.includes(series.Id)),
    rail: rail.map((entry) => entry.name),
    selected: selected?.Name ?? null,
    episodes,
    shown: shown?.length ?? null,
    liveSeries: servedChildrenLive(log, series.Id),
    liveSeason: selected ? servedChildrenLive(log, selected.Id) : [],
  }
  await ctx.screenshot("07a-uncached-series")
  expect(result.servedUncached && sameList(result.rail, seasonRailNames(seasons.map((season) => ({ name: season.Name, indexNumber: season.IndexNumber })))) && shown !== null, `uncached series ${series.Name}: served past the cache, its page lists its ${seasons.length} seasons and the ${episodes} episodes of ${result.selected} (uncached ${result.servedUncached}, rail ${JSON.stringify(result.rail)}, shown ${result.shown})`)
  expect(result.liveSeries.length === 0 && result.liveSeason.length === 0, `uncached series ${series.Name}: its seasons and episodes come from the cache, not live from the server (${JSON.stringify([...result.liveSeries, ...result.liveSeason])})`)
  return result
}

async function seasonRail(ctx) {
  // The label, not the button's whole text: art-less posters repeat the name.
  return ctx.app.evaluate(`[...document.querySelectorAll('[aria-label="Seasons"] button')].map((button) => ({ name: button.lastElementChild.textContent.trim(), pressed: button.getAttribute("aria-pressed") === "true" }))`)
}

async function episodeLinks(ctx) {
  return ctx.app.evaluate(`[...new Set([...document.querySelectorAll('a[href^="/item/"]:has(> span.tabular-nums)')].map((link) => link.getAttribute("href")))]`)
}

async function seasonEpisodes(ctx, seasonId) {
  return (await ctx.api(`/api/item/${seasonId}/children`)).body.items.filter((child) => child.kind === "Episode")
}

async function seriesAndSeason(ctx, expect) {
  const candidates = (await ctx.api("/api/items?kind=Series&sort=name&limit=60")).body.items
  const series = candidates.find((item) => (item.childCount ?? 0) > 1) ?? candidates.find((item) => (item.childCount ?? 0) > 0)
  expect(Boolean(series), `a series with seasons (${series?.name})`)
  if (!series) return null
  const seasons = (await ctx.api(`/api/item/${series.id}/children`)).body.items.filter((child) => child.kind === "Season")
  const nextUp = (await ctx.api(`/api/item/${series.id}/nextup`)).body?.item ?? null
  await openByUrl(ctx, `/item/${series.id}`)
  await ctx.find({ role: "heading", name: series.name }, { timeout: 30000 })
  await ctx.find({ role: "list", name: "Seasons" }, { timeout: 30000 })
  await ctx.find({ role: "heading", name: "Episodes" })
  const rail = await ctx.until(async () => { const found = await seasonRail(ctx); return found.some((season) => season.pressed) ? found : null }, "a selected season", 15000)
  const selected = seasons.find((season) => season.name === rail.find((entry) => entry.pressed)?.name)
  const expected = selected ? await seasonEpisodes(ctx, selected.id) : []
  const shown = await ctx.until(async () => { const links = await episodeLinks(ctx); return links.length === expected.length ? links : null }, "the season's episode list", 15000).catch(() => null)
  const result = { id: series.id, name: series.name, seasons: seasons.map((season) => season.name), rail, selected: selected?.name ?? null, episodes: expected.length, shown: shown?.length ?? null }
  expect(sameList(rail.map((entry) => entry.name), seasonRailNames(seasons)), `series ${series.name}: the season rail lists its seasons (${JSON.stringify(rail.map((entry) => entry.name))})`)
  expect(expected.length > 0 && shown !== null, `series ${series.name}: the episode list shows the ${expected.length} episodes of ${selected?.name} (${shown?.length ?? "a different number"})`)
  if (nextUp) {
    const code = `S${nextUp.parentIndexNumber}E${nextUp.indexNumber}`
    result.playLabel = await ctx.until(async () => {
      for (const label of [`Play ${code}`, `Resume ${code}`]) if (await ctx.exists({ role: "button", name: label })) return label
      return null
    }, "the series' Play button", 15000).catch(() => null)
    result.nextUpNote = (await ctx.app.evaluate("document.body.innerText")).includes(`Next up: ${code}`)
    expect(result.playLabel !== null && result.nextUpNote, `series ${series.name}: the Play button and the note name Next up ${code} (${result.playLabel}, note ${result.nextUpNote})`)
  } else {
    ctx.step(`series ${series.name}: no Next Up episode for this user; the Play label is not checked`)
  }
  await ctx.screenshot("07-series")

  const season = seasons.find((entry) => entry.id !== selected?.id) ?? selected
  if (season) {
    await ctx.app.evaluate(`history.pushState(null, "", ${JSON.stringify(`/item/${season.id}`)}); dispatchEvent(new PopStateEvent("popstate"))`)
    await ctx.until(async () => (await ctx.pathname()) === `/item/${encodeURIComponent(series.id)}?season=${encodeURIComponent(season.id)}`, "the season page to open its series on that season", 30000)
    const pressed = await ctx.until(async () => (await seasonRail(ctx)).find((entry) => entry.name === season.name)?.pressed || null, `${season.name} to be selected`, 15000).catch(() => false)
    const episodes = await seasonEpisodes(ctx, season.id)
    const listed = await ctx.until(async () => { const links = await episodeLinks(ctx); return links.length === episodes.length ? links : null }, `the ${season.name} episodes`, 15000).catch(() => null)
    result.season = { id: season.id, name: season.name, pressed: Boolean(pressed), episodes: episodes.length, shown: listed?.length ?? null }
    expect(Boolean(pressed) && episodes.length > 0 && listed !== null, `season ${season.name}: its page opens the series on that season with its ${episodes.length} episodes (selected ${Boolean(pressed)}, shown ${listed?.length ?? "a different number"})`)
    await ctx.screenshot("08-season")
  }
  return result
}

// ui/src/lib/seasons.ts: Specials (season 0) go last.
function seasonRailNames(seasons) {
  return [
    ...seasons.filter((season) => season.indexNumber !== 0),
    ...seasons.filter((season) => season.indexNumber === 0),
  ].map((season) => season.name)
}

async function collectionChip(ctx, expect, userId) {
  const localDate = new Date().toLocaleDateString("sv-SE")
  const list = await ctx.until(async () => {
    const body = (await ctx.api(`/api/collections/franchises?localDate=${localDate}`)).body
    return body?.status === "ready" ? body : null
  }, "the franchise snapshot", 180000).catch(() => null)
  expect(Boolean(list), "the account's movie franchises are ready")
  if (!list) return null
  let movie = null
  let franchise = null
  for (const candidate of list.franchises.filter((entry) => entry.ownedCount > 0)) {
    const detail = (await ctx.api(`/api/collections/franchises/${candidate.collectionId}?localDate=${localDate}`)).body
    movie = detail?.libraryItems?.find((item) => item.kind === "Movie") ?? null
    if (movie) { franchise = candidate; break }
  }
  expect(Boolean(movie), `a movie that belongs to a franchise (${movie?.name}, ${franchise?.name})`)
  if (!movie) return null
  const server = await ctx.jellyfin(`/Items/${movie.id}?userId=${userId}`)
  await openByUrl(ctx, `/item/${movie.id}`)
  await ctx.find({ role: "heading", name: movie.name }, { timeout: 30000 })
  const chip = await ctx.until(() => ctx.app.evaluate(`document.querySelector(${JSON.stringify(HERO)}).querySelector('a[href^="/collections/franchises/"]')?.textContent.replace(/\\s+/g, " ").trim() ?? null`), "the collection chip", 30000).catch(() => null)
  await ctx.screenshot("09-collection-chip")
  expect(server?.Id === movie.id && chip === `Part of ${franchise.name}`, `movie ${movie.name}: Jellyfin has it and its title page shows the "Part of ${franchise.name}" chip (${chip})`)
  return { id: movie.id, name: movie.name, franchise: franchise.name, chip }
}

async function titlePage(ctx, expect, { label, id, name }) {
  await ctx.find({ role: "heading", name }, { timeout: 30000 })
  await ctx.until(async () => (await ctx.exists({ role: "button", name: "Play" })) || (await ctx.exists({ role: "button", name: "Resume" })), `${label}: the Play or Resume button`)
  await ctx.find({ role: "combobox", name: "Streaming quality" })
  const detail = await ctx.api(`/api/item/${id}`)
  const found = await surfaces(ctx)
  expect(detail.body?.layout === "title", `${label}: /api/item answers the title layout (${detail.body?.layout})`)
  expect(found.controls.includes("More info"), `${label}: the More info menu is there`)
  expect(found.controls.some((control) => /^(Add to|Remove from) favorites$/.test(control)) && found.controls.some((control) => /^Mark (un)?watched$/.test(control)), `${label}: the favorites and watched toggles are there`)
  expect(found.posterFit === null || found.posterFit === "cover", `${label}: the poster still fills its 2:3 frame (${found.posterFit})`)
  return { id, name, detail: detail.body, surfaces: found }
}

export default async function genericDetail(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  const report = { mode: ctx.account ? `account ${ctx.account.server}` : "demo", checks: [] }
  const expect = (condition, message) => {
    report.checks.push({ ok: Boolean(condition), message })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}`)
  }
  const { userId } = (await ctx.api("/api/status")).body
  report.uncachedTitle = await uncachedTitle(ctx, expect, userId)
  report.uncachedSeries = await uncachedSeries(ctx, expect, userId)
  await ctx.until(async () => {
    const status = (await ctx.api("/api/status")).body
    return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
  }, "the first sync to finish", 300000)

  const views = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items
  const libraries = (await ctx.api("/api/libraries")).body.libraries

  report.contract = { malformedId: {} }
  for (const route of ["", "/children", "/about", "/synopsis"]) {
    report.contract.malformedId[route || "/"] = (await ctx.api(`/api/item/not-an-item${route}`)).status
  }
  expect(Object.values(report.contract.malformedId).every((status) => status === 400), `a malformed item id answers 400 on the detail, children, about and synopsis routes (${JSON.stringify(report.contract.malformedId)})`)

  const music = views.find((view) => view.CollectionType === "music")
  if (music) {
    const albums = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${music.Id}&Recursive=true&IncludeItemTypes=MusicAlbum&SortBy=SortName&Fields=ChildCount,PrimaryImageAspectRatio&Limit=100`)
    const album = albums.Items.find((item) => item.ImageTags?.Primary && item.ChildCount > 0)
    expect(Boolean(album), `${music.Name}: an album with artwork and tracks (${album?.Name})`)
    if (album) {
      const folder = await genericFolder(ctx, expect, { label: `album ${album.Name}`, id: album.Id, name: album.Name, userId, childCount: album.ChildCount })
      expect(folder.cards === album.ChildCount, `album ${album.Name}: ${folder.cards} track cards equal its ChildCount ${album.ChildCount}`)
      expect(folder.surfaces.posterFit === "contain" && Math.abs(folder.surfaces.posterBox - 1) < 0.02, `album ${album.Name}: the cover is contained in a square hero frame (${folder.surfaces.posterFit}, ${folder.surfaces.posterBox})`)
      const fitOf = (item) => folder.seen.get(item.id)?.fit
      const squareCards = folder.server.items.filter((item) => item.aspect != null && item.aspect >= SQUARE_ART.min && item.aspect <= SQUARE_ART.max)
      const wideCards = folder.server.items.filter((item) => item.aspect != null && item.aspect > SQUARE_ART.max)
      const uncontained = squareCards.filter((item) => fitOf(item) !== "contain").map((item) => `${item.name}: ${fitOf(item)}`)
      const unfilled = wideCards.filter((item) => fitOf(item) !== "cover").map((item) => `${item.name}: ${fitOf(item)}`)
      if (squareCards.length) {
        expect(uncontained.length === 0, `album ${album.Name}: ${squareCards.length} track cards with near-square art use object-contain (${JSON.stringify(uncontained)})`)
      } else {
        ctx.step(`album ${album.Name}: no track has near-square art of its own; card containment not exercised`)
      }
      if (wideCards.length) {
        expect(unfilled.length === 0, `album ${album.Name}: ${wideCards.length} track cards with wide art fill the frame (${JSON.stringify(unfilled)})`)
      }
      await ctx.screenshot("01-album")
      await ctx.snapshot("01-album")

      report.contract.negativeOffset = (await ctx.api(`/api/item/${album.Id}/children?offset=-1`)).status
      const clamped = await ctx.api(`/api/item/${album.Id}/children?limit=100000`)
      report.contract.clampedLimit = { status: clamped.status, items: clamped.body?.items?.length, total: clamped.body?.total }
      expect(report.contract.negativeOffset === 400, `a negative children offset answers 400 (${report.contract.negativeOffset})`)
      expect(clamped.status === 200 && clamped.body.items.length === Math.min(clamped.body.total, LIVE_PAGE_MAX), `limit=100000 is clamped to ${LIVE_PAGE_MAX} (${clamped.body?.items?.length} of ${clamped.body?.total})`)

      const track = folder.server.items[0]
      await ctx.press({ css: `${GRID_CARD_LINK}[href$="/item/${encodeURIComponent(track.id)}"]` })
      await ctx.until(async () => (await ctx.pathname()) === `/item/${encodeURIComponent(track.id)}`, `the ${track.name} page`)
      await ctx.find({ role: "heading", name: track.name }, { timeout: 30000 })
      const trackDetail = await ctx.api(`/api/item/${track.id}`)
      const trackSurfaces = await surfaces(ctx)
      expect(trackDetail.body?.layout === "generic" && trackDetail.body.isFolder === false, `track ${track.name}: /api/item answers the generic layout, not a folder (${trackDetail.body?.layout}, isFolder=${trackDetail.body?.isFolder})`)
      noTitleSurfaces(trackSurfaces, `track ${track.name}`, expect)
      expect(!trackSurfaces.contents, `track ${track.name}: no Contents grid`)
      await ctx.screenshot("02-track")
      const { seen: _seen, server, ...albumReport } = folder
      report.album = { ...albumReport, serverChildren: server.items, track: { ...track, back: trackSurfaces.back } }
    }

    const artist = (await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${music.Id}&IncludeItemTypes=MusicArtist&SortBy=SortName&Fields=ChildCount&Limit=20`)).Items.find((item) => item.ChildCount > 0)
    if (artist) {
      const byArtist = await ctx.jellyfin(`/Items?userId=${userId}&ArtistIds=${artist.Id}&IncludeItemTypes=MusicAlbum&Recursive=true&Limit=0`)
      const folder = await genericFolder(ctx, expect, { label: `artist ${artist.Name}`, id: artist.Id, name: artist.Name, userId, childCount: artist.ChildCount })
      expect(folder.cards === artist.ChildCount && folder.cards > 0, `artist ${artist.Name}: the grid lists its ${artist.ChildCount} albums, read by ParentId (an ArtistIds query finds ${byArtist.TotalRecordCount})`)
      await ctx.screenshot("02b-artist")
      const { seen: _seen, server, ...artistReport } = folder
      report.artist = { ...artistReport, serverChildren: server.items, artistIdsAlbums: byArtist.TotalRecordCount }
    } else {
      ctx.step(`${music.Name}: no MusicArtist folder with albums; the artist grid is not exercised`)
    }
  } else if (ctx.account) {
    ctx.step("this server has no Music library; the album and artist checks are not exercised")
  } else {
    expect(false, "the demo server has a Music library")
  }

  const moviesView = views.find((view) => view.CollectionType === "movies")
  if (moviesView) {
    const folder = await genericFolder(ctx, expect, { label: `library folder ${moviesView.Name}`, id: moviesView.Id, name: moviesView.Name, userId, childCount: null })
    expect(folder.cards > 0, `library folder ${moviesView.Name}: lists ${folder.cards} entries, paged in while the page scrolls`)
    await ctx.screenshot("03-library-folder")
    const clamped = await ctx.api(`/api/item/${moviesView.Id}/children?limit=100000`)
    report.contract.folderClampedLimit = { status: clamped.status, items: clamped.body?.items?.length, total: clamped.body?.total }
    expect(clamped.status === 200 && clamped.body.items.length === Math.min(clamped.body.total, LIVE_PAGE_MAX), `library folder ${moviesView.Name}: limit=100000 is clamped to ${LIVE_PAGE_MAX} (${clamped.body?.items?.length} of ${clamped.body?.total})`)
    const { seen: _seen, server: _server, ...folderReport } = folder
    report.libraryFolder = folderReport
  }

  const top = (await ctx.api("/api/items?kind=Movie&sort=rating&order=desc&limit=1")).body.items[0]
  expect(Boolean(top) && top.communityRating != null, `the top-rated movie has a community rating (${top?.name}, ${top?.communityRating})`)
  await openFromSearch(ctx, top.name)
  const movie = await titlePage(ctx, expect, { label: `movie ${top.name}`, id: top.id, name: top.name })
  const movieLibrary = libraries.find((library) => library.id === movie.detail.libraryId)
  expect(movie.surfaces.communityRating, `movie ${top.name}: its community rating shows`)
  expect(movieLibrary?.kind === "movies", `movie ${top.name}: libraryId names a movies library (${movie.detail.libraryId}, ${movieLibrary?.name})`)
  expect(movie.surfaces.back === "Back to library", `movie ${top.name}: the back link returns to the search (${movie.surfaces.back})`)
  ctx.step(`movie ${top.name}: ratings readout ${movie.surfaces.ratings}, collection chip ${movie.surfaces.collectionChip}`)
  await ctx.screenshot("04-movie")
  report.movie = { id: movie.id, name: movie.name, libraryId: movie.detail.libraryId, library: movieLibrary?.name ?? null, surfaces: movie.surfaces }

  const episode = (await ctx.api("/api/items?kind=Episode&limit=1")).body.items[0]
  expect(Boolean(episode), `the catalog holds an episode (${episode?.name})`)
  if (episode) {
    const episodeDetail = (await ctx.api(`/api/item/${episode.id}`)).body
    const seriesDetail = (await ctx.api(`/api/item/${episode.seriesId}`)).body
    const episodeLibrary = libraries.find((library) => library.id === episodeDetail.libraryId)
    report.episode = { id: episode.id, name: episode.name, layout: episodeDetail.layout, libraryId: episodeDetail.libraryId, seriesLibraryId: seriesDetail.libraryId, library: episodeLibrary?.name ?? null }
    expect(episodeDetail.layout === "title" && episodeLibrary?.kind === "shows" && episodeDetail.libraryId === seriesDetail.libraryId, `episode ${episode.name}: the title layout reports its series' TV shows library (${episodeDetail.libraryId}, series ${seriesDetail.libraryId})`)
  }

  report.series = await seriesAndSeason(ctx, expect)

  if (ctx.account) {
    report.collectionChip = await collectionChip(ctx, expect, userId)
    const collections = views.find((view) => view.CollectionType === "boxsets")
    expect(Boolean(collections), `the account has a Collections library (${collections?.Name})`)
    if (collections) {
      const sets = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${collections.Id}&Recursive=true&IncludeItemTypes=BoxSet&SortBy=SortName&Fields=ChildCount&Limit=100`)
      const set = sets.Items.reduce((most, item) => (most && most.ChildCount >= item.ChildCount ? most : item), null)
      expect(set?.ChildCount > 0, `${collections.Name}: a box set with titles (${set?.Name}, ${set?.ChildCount})`)
      if (set?.ChildCount > 0) {
        const folder = await genericFolder(ctx, expect, { label: `box set ${set.Name}`, id: set.Id, name: set.Name, userId, childCount: set.ChildCount })
        await ctx.screenshot("05-box-set")
        report.boxSetRemoteWatched = await remoteWatchedFollows(ctx, expect, { label: `box set ${set.Name}`, setId: set.Id, userId })
        report.boxSetFavorite = await favoriteFromCard(ctx, expect, { label: `box set ${set.Name}`, setId: set.Id, userId })
        const first = folder.server.items.find((item) => item.type === "Movie")
        expect(Boolean(first), `box set ${set.Name}: holds a movie (${first?.name})`)
        if (first) {
          await ctx.press({ css: `${GRID_CARD_LINK}[href$="/item/${encodeURIComponent(first.id)}"]` })
          await ctx.until(async () => (await ctx.pathname()).startsWith(`/item/${encodeURIComponent(first.id)}`), `the ${first.name} page`)
          const title = await titlePage(ctx, expect, { label: `box set title ${first.name}`, id: first.id, name: first.name })
          await ctx.screenshot("06-box-set-title")
          report.boxSetTitle = { id: title.id, name: title.name, kind: title.detail?.kind, libraryId: title.detail?.libraryId, back: title.surfaces.back }
        }
        const { seen: _seen, server, ...setReport } = folder
        report.boxSet = { ...setReport, serverChildren: server.items }
      }
    }
  }

  ctx.writeEvidence("generic-detail.json", report)
  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
