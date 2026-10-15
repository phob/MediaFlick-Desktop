// Generic details pages, read-only. Nothing links to an album or a box set's
// own page until the library browse change, so the drive opens them by URL
// with no navigation state, as a reload or deep link does.
// Both modes: the Music library's first album with artwork and tracks shows
// no title controls (Play, favorites, watched, More info, quality, ratings,
// collection chip, Letterboxd, Media), goes back to Home, contains its cover
// in the hero and any square track art in the cards, and its grid holds
// exactly the tracks Jellyfin lists, as many as the album's ChildCount; a
// track opens a generic page with no contents grid. The Movies library's own
// folder page lists exactly its direct children, paged in as the page
// scrolls. Then the top-rated movie, opened from a sidebar search, keeps its
// title page: Play or Resume, quality, watched and favorites, More info, its
// community rating and its library. An episode reports its series' library.
// The children route rejects a bad id and a negative offset and clamps its
// limit.
// With --account only: the Collections library's largest box set lists
// exactly its titles; a movie card that is not a favorite is favorited from
// its preview and shows it on the next hover without leaving the page, then
// is unfavorited the same way (journaled after-run fallback); and a movie
// card opens the full movie page.
//   just verify generic-detail
//   just verify generic-detail --account https://jellyfin.example.com
const DEMO = "https://demo.jellyfin.org/stable"
const HERO = '[data-testid="detail-hero-layout"]'
const GRID_CARD_LINK = '[data-item-grid-canvas] article a[aria-label^="Open details for"]'
const TITLE_CONTROLS = /^(Play|Resume|Play S\d+E\d+|Resume S\d+E\d+|Add to favorites|Remove from favorites|Mark watched|Mark unwatched|More info|Streaming quality)$/
const SQUARE_ART = 0.8
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
  const views = (await ctx.jellyfin(`/UserViews?userId=${userId}`)).Items
  const libraries = (await ctx.api("/api/libraries")).body.libraries

  report.contract = {
    malformedDetail: (await ctx.api("/api/item/not-an-item")).status,
    malformedChildren: (await ctx.api("/api/item/not-an-item/children")).status,
  }
  expect(report.contract.malformedDetail === 400 && report.contract.malformedChildren === 400, `a malformed item id answers 400 on the detail and children routes (${report.contract.malformedDetail}, ${report.contract.malformedChildren})`)

  const music = views.find((view) => view.CollectionType === "music")
  if (music) {
    const albums = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${music.Id}&Recursive=true&IncludeItemTypes=MusicAlbum&SortBy=SortName&Fields=ChildCount,PrimaryImageAspectRatio&Limit=100`)
    const album = albums.Items.find((item) => item.ImageTags?.Primary && item.ChildCount > 0)
    expect(Boolean(album), `${music.Name}: an album with artwork and tracks (${album?.Name})`)
    if (album) {
      const folder = await genericFolder(ctx, expect, { label: `album ${album.Name}`, id: album.Id, name: album.Name, userId, childCount: album.ChildCount })
      expect(folder.cards === album.ChildCount, `album ${album.Name}: ${folder.cards} track cards equal its ChildCount ${album.ChildCount}`)
      expect(folder.surfaces.posterFit === "contain" && Math.abs(folder.surfaces.posterBox - 1) < 0.02, `album ${album.Name}: the cover is contained in a square hero frame (${folder.surfaces.posterFit}, ${folder.surfaces.posterBox})`)
      const squareCards = folder.server.items.filter((item) => (item.aspect ?? 0) >= SQUARE_ART)
      const uncontained = squareCards.filter((item) => folder.seen.get(item.id)?.fit !== "contain").map((item) => `${item.name}: ${folder.seen.get(item.id)?.fit}`)
      if (squareCards.length) {
        expect(uncontained.length === 0, `album ${album.Name}: ${squareCards.length} track cards with square art use object-contain (${JSON.stringify(uncontained)})`)
      } else {
        ctx.step(`album ${album.Name}: no track has square art of its own; card containment not exercised`)
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
  } else {
    expect(Boolean(ctx.account), "the demo server has a Music library")
    ctx.step("this server has no Music library; the album checks are not exercised")
  }

  const moviesView = views.find((view) => view.CollectionType === "movies")
  if (moviesView) {
    const folder = await genericFolder(ctx, expect, { label: `library folder ${moviesView.Name}`, id: moviesView.Id, name: moviesView.Name, userId, childCount: null })
    expect(folder.cards > 0, `library folder ${moviesView.Name}: lists ${folder.cards} entries, paged in while the page scrolls`)
    await ctx.screenshot("03-library-folder")
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

  if (ctx.account) {
    const collections = views.find((view) => view.CollectionType === "boxsets")
    expect(Boolean(collections), `the account has a Collections library (${collections?.Name})`)
    if (collections) {
      const sets = await ctx.jellyfin(`/Items?userId=${userId}&ParentId=${collections.Id}&Recursive=true&IncludeItemTypes=BoxSet&SortBy=SortName&Fields=ChildCount&Limit=100`)
      const set = sets.Items.reduce((most, item) => (most && most.ChildCount >= item.ChildCount ? most : item), null)
      expect(set?.ChildCount > 0, `${collections.Name}: a box set with titles (${set?.Name}, ${set?.ChildCount})`)
      if (set?.ChildCount > 0) {
        const folder = await genericFolder(ctx, expect, { label: `box set ${set.Name}`, id: set.Id, name: set.Name, userId, childCount: set.ChildCount })
        await ctx.screenshot("05-box-set")
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
