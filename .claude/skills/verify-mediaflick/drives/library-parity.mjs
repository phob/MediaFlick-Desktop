const DEMO = "https://demo.jellyfin.org/stable"
const AUTH = 'MediaBrowser Client="verify", Device="verify", DeviceId="verify-parity", Version="1"'
const LETTERS = ["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"]
const FIRST_TITLES = 6
const itemCount = (ctx) => ctx.app.evaluate(`(() => { const m = document.body.innerText.match(/^([\\d,]+) items?$/m); return m ? Number(m[1].replace(/,/g, "")) : null })()`)

// The demo account is shared, so strangers change watch state while the drive
// runs. A comparison passes once both sides agree within the window.
const LIVE_WINDOW_MS = 90000

async function jellyfinClient() {
  const response = await fetch(`${DEMO}/Users/AuthenticateByName`, {
    method: "POST",
    headers: { Authorization: AUTH, "Content-Type": "application/json" },
    body: JSON.stringify({ Username: "demo", Pw: "" }),
  })
  if (!response.ok) throw new Error(`demo sign-in failed: ${response.status}`)
  const session = await response.json()
  const header = `${AUTH}, Token="${session.AccessToken}"`
  return async (path, params = {}) => {
    const url = new URL(`${DEMO}${path}`)
    url.searchParams.set("userId", session.User.Id)
    for (const [key, value] of Object.entries(params)) url.searchParams.set(key, String(value))
    const reply = await fetch(url, { headers: { Authorization: header } })
    if (!reply.ok) throw new Error(`${url.pathname} answered ${reply.status}`)
    return reply.json()
  }
}

function seriesInProgressFromEpisodes(episodes) {
  const bySeries = new Map()
  for (const episode of episodes) {
    const entry = bySeries.get(episode.SeriesId) ?? { started: false, unwatched: false }
    if (episode.UserData?.Played || episode.UserData?.PlaybackPositionTicks > 0) entry.started = true
    if (!episode.UserData?.Played && episode.LocationType !== "Virtual") entry.unwatched = true
    bySeries.set(episode.SeriesId, entry)
  }
  return [...bySeries.values()].filter((entry) => entry.started && entry.unwatched).length
}

export default async function libraryParity(ctx) {
  const report = { server: DEMO, comparisons: [] }
  const jellyfin = await jellyfinClient()
  const serverCount = async (params) =>
    (await jellyfin("/Items", { Recursive: true, Limit: 0, ...params })).TotalRecordCount
  const appItems = async (query) => {
    const reply = await ctx.api(`/api/items?${query}&limit=500`)
    ctx.check(reply.status === 200, `/api/items?${query} answers`)
    return reply.body
  }

  async function agree(label, read) {
    const deadline = Date.now() + LIVE_WINDOW_MS
    let attempts = 0
    let last
    while (Date.now() < deadline) {
      attempts += 1
      last = await read()
      if (JSON.stringify(last.app) === JSON.stringify(last.server)) {
        report.comparisons.push({ label, ...last, attempts })
        ctx.check(true, `${label}: app ${JSON.stringify(last.app)} = server ${JSON.stringify(last.server)}`)
        return last
      }
      await new Promise((resolve) => setTimeout(resolve, 3000))
    }
    report.comparisons.push({ label, ...last, attempts, agreed: false })
    throw new Error(`${label}: app ${JSON.stringify(last.app)} != server ${JSON.stringify(last.server)}`)
  }

  async function firstTitles(name) {
    const tree = await ctx.snapshot(name)
    return [...tree.matchAll(/link 'Open details for ([^']*)'/g)].map((match) => match[1]).slice(0, FIRST_TITLES)
  }
  async function gridReady(description) {
    const ready = await ctx.until(async () => {
      const count = await itemCount(ctx)
      const failed = await ctx.exists({ role: "heading", name: "Could not load library items" })
      if (failed) throw new Error(`the grid failed to load for ${description}`)
      const cards = await ctx.count({ css: 'a[aria-label^="Open details for"]' })
      return count !== null && (count === 0 || cards > 0) ? { count } : null
    }, `the grid for ${description}`, 30000)
    return ready.count
  }
  async function sortOptions() {
    await ctx.press({ role: "combobox", name: "Sort by" })
    await ctx.find({ role: "listbox" })
    const tree = await ctx.snapshot("sort-options")
    await ctx.key("Escape")
    await ctx.until(async () => (await ctx.state({ role: "combobox", name: "Sort by" })).expanded !== true, "the sort list to close", 5000)
    return [...tree.matchAll(/option '([^']*)'/g)].map((match) => match[1])
  }
  async function openSubmenu(prefix, snapshotName) {
    const trigger = await ctx.until(async () => {
      const name = (await ctx.app.evaluate(`[...document.querySelectorAll('[role="menuitem"]')].map((node) => node.textContent.trim().replace(/\\s+/g, " "))`))
        .find((text) => text.startsWith(prefix))
      return name ?? null
    }, `the ${prefix} submenu trigger`, 10000)
    const menu = await ctx.snapshot(snapshotName)
    const exact = [...menu.matchAll(/menuitem '([^']*)'/g)].map((match) => match[1]).find((name) => name.startsWith(prefix)) ?? trigger
    await ctx.press({ role: "menuitem", name: exact })
    const options = await ctx.snapshot(`${snapshotName}-options`)
    return [...options.matchAll(/menuitemradio '([^']*)'/g)].map((match) => match[1])
  }
  async function openFilters() {
    const name = await ctx.until(async () => {
      const labels = await ctx.app.evaluate(`[...document.querySelectorAll('button[aria-label^="Filters"]')].map((node) => node.getAttribute("aria-label"))`)
      return labels[0] ?? null
    }, "the Filters button", 10000)
    await ctx.press({ role: "button", name })
    await ctx.find({ role: "menu", name })
  }
  async function menuNames() {
    const tree = await ctx.snapshot("filters-menu")
    return [...tree.matchAll(/menuitem '([^']*)'/g)].map((match) => match[1])
  }
  async function closeMenus() {
    while ((await ctx.count({ role: "menu" })) > 0) await ctx.key("Escape")
  }

  async function jumpResult(index) {
    return ctx.app.evaluate(`(() => {
      const canvas = document.querySelector("[data-item-grid-canvas]")
      const scroller = canvas.closest(".overflow-y-auto")
      const rows = [...canvas.querySelectorAll(".virtual-card-row")]
      const cards = rows.flatMap((row) => [...row.querySelectorAll('a[aria-label^="Open details for"]')])
      const top = scroller.getBoundingClientRect().top
      const visible = cards.filter((card) => card.getBoundingClientRect().bottom > top + 1)
      const columns = Math.max(...rows.map((row) => row.querySelectorAll("article").length))
      const targetRow = Math.floor(${index} / columns)
      const rowStarts = rows.map((candidate) => Number(candidate.style.transform.match(/translateY\\(([\\d.]+)px\\)/)?.[1])).sort((a, b) => a - b)
      const rowHeight = rowStarts.length > 1 ? Math.min(...rowStarts.slice(1).map((start, i) => start - rowStarts[i]).filter((gap) => gap > 0)) : 0
      return {
        scrollTop: scroller.scrollTop,
        maxScroll: scroller.scrollHeight - scroller.clientHeight,
        columns,
        targetRow,
        targetRowStart: targetRow * rowHeight,
        firstVisible: visible[0]?.getAttribute("aria-label").replace("Open details for ", "") ?? null,
      }
    })()`)
  }

  async function checkRail(kind, query, direction, label) {
    const letters = (await ctx.api(`/api/items/letters?${query}`)).body.letters
    const items = (await appItems(query)).items
    const server = await jellyfin("/Items", { Recursive: true, IncludeItemTypes: kind, Fields: "SortName" })
    const sortNames = new Map(server.Items.map((item) => [item.Id, item.SortName ?? item.Name]))
    const letterOf = (item) => {
      const initial = (sortNames.get(item.id) ?? item.name).charAt(0).toUpperCase()
      return /^[A-Z]$/.test(initial) ? initial : "#"
    }
    const expected = {}
    items.forEach((item, index) => { const letter = letterOf(item); if (!(letter in expected)) expected[letter] = index })
    ctx.check(JSON.stringify(Object.entries(letters).sort()) === JSON.stringify(Object.entries(expected).sort()),
      `${label}: /api/items/letters ${JSON.stringify(letters)} matches the /api/items order read with the server's SortName`)
    const states = {}
    for (const letter of LETTERS) {
      states[letter] = (await ctx.state({ role: "button", name: `Jump to ${letter}` })).disabled === true
      ctx.check(states[letter] === !(letter in letters), `${label}: Jump to ${letter} is ${states[letter] ? "disabled" : "enabled"}`)
    }
    const present = Object.keys(letters)
    const target = present.reduce((best, letter) => (letters[letter] > letters[best] ? letter : best), present[0])
    const index = letters[target]
    await ctx.press({ role: "button", name: `Jump to ${target}` })
    const landed = await ctx.until(async () => {
      const result = await jumpResult(index)
      const wanted = Math.min(result.targetRowStart, result.maxScroll)
      return Math.abs(result.scrollTop - wanted) <= 2 ? result : null
    }, `${label}: the grid to scroll to row of index ${index}`, 10000)
    const indexed = items[index]
    ctx.check(letterOf(indexed) === target, `${label}: the item at index ${index} (${indexed.name}) starts with ${target} by its server SortName`)
    if (landed.maxScroll >= landed.targetRowStart) {
      ctx.check(landed.firstVisible === items[landed.targetRow * landed.columns].name, `${label}: ${landed.firstVisible} heads the grid after Jump to ${target}`)
    } else {
      ctx.check(landed.scrollTop === landed.maxScroll, `${label}: the grid scrolled as far as it can (${landed.maxScroll}px) towards row ${landed.targetRow}`)
    }
    const first = present.find((letter) => letters[letter] === 0)
    await ctx.press({ role: "button", name: `Jump to ${first}` })
    const top = await ctx.until(async () => {
      const result = await jumpResult(0)
      return result.scrollTop === 0 ? result : null
    }, `${label}: Jump to ${first} to scroll to the top`, 10000)
    ctx.check(top.firstVisible === items[0].name && letterOf(items[0]) === first,
      `${label}: after Jump to ${first}, the first visible card is ${top.firstVisible}, filed under ${first}`)
    return {
      direction,
      letters,
      expected,
      disabled: states,
      jumped: { letter: target, index, item: indexed.name, ...landed },
      jumpedBack: { letter: first, ...top },
    }
  }

  await ctx.signIn({ server: DEMO, username: "demo" })

  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?kind=Movie", "the Movies library")
  await ctx.park()
  await gridReady("Movies")
  report.movieSorts = await sortOptions()
  for (const label of ["Sort: Name", "Sort: Release date", "Sort: Date added", "Sort: Rating", "Sort: Date played", "Sort: Play count", "Sort: Runtime", "Sort: Random"]) {
    ctx.check(report.movieSorts.includes(label), `Movies offer ${label}`)
  }
  ctx.check(!report.movieSorts.includes("Sort: Date episode added"), "Movies do not offer Sort: Date episode added")

  for (const [label, id] of [["Sort: Release date", "year"], ["Sort: Date added", "added"], ["Sort: Date played", "played"], ["Sort: Play count", "plays"], ["Sort: Runtime", "runtime"], ["Sort: Name", "name"]]) {
    await ctx.choose({ name: "Sort by" }, label)
    await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get("sort") === id, `sort=${id} in the URL`)
    await gridReady(label)
  }

  const ascending = await firstTitles("01-movies-name-asc")
  const apiAscending = (await appItems("kind=Movie&sort=name")).items.map((item) => item.name)
  const apiDescending = (await appItems("kind=Movie&sort=name&order=desc")).items.map((item) => item.name)
  ctx.check(JSON.stringify(ascending) === JSON.stringify(apiAscending.slice(0, ascending.length)), "the grid shows /api/items' ascending name order")
  ctx.check(JSON.stringify(apiDescending) === JSON.stringify([...apiAscending].reverse()), "descending name order is the ascending order reversed")
  await ctx.press({ role: "button", name: "Sort descending" })
  await ctx.until(async () => (await ctx.pathname()).includes("order=desc"), "order=desc in the URL")
  await ctx.find({ role: "button", name: "Sort ascending" })
  await ctx.until(async () => (await firstTitles("02-movies-name-desc"))[0] === apiDescending[0], "the grid to reverse")
  const descending = await firstTitles("02-movies-name-desc")
  ctx.check(JSON.stringify(descending) === JSON.stringify(apiDescending.slice(0, descending.length)), `the first titles reverse: ${descending.join(", ")}`)
  report.direction = { ascending, descending, url: await ctx.pathname() }

  report.railMoviesDesc = await checkRail("Movie", "kind=Movie&sort=name&order=desc", "desc", "Movies A–Z descending")
  await ctx.park()
  await ctx.screenshot("03-movies-rail-desc")
  await ctx.press({ role: "button", name: "Sort ascending" })
  await ctx.until(async () => !(await ctx.pathname()).includes("order="), "order to leave the URL at the natural direction")
  await ctx.until(async () => (await firstTitles("04-movies-name-asc-again"))[0] === apiAscending[0], "the grid to read ascending again")
  report.railMoviesAsc = await checkRail("Movie", "kind=Movie&sort=name", "asc", "Movies A–Z ascending")

  await ctx.choose({ name: "Sort by" }, "Sort: Random")
  const seed = await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get("seed"), "a seed in the URL")
  ctx.check(!(await ctx.exists({ role: "navigation", name: "Jump to letter" })), "the A–Z rail leaves with the name sort")
  ctx.check(!(await ctx.exists({ role: "button", name: "Sort descending" })) && !(await ctx.exists({ role: "button", name: "Sort ascending" })), "Random has no direction toggle")
  await gridReady("Sort: Random")
  const shuffled = await firstTitles("05-movies-random")
  await ctx.app.evaluate("location.reload()")
  await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get("seed") === seed, "the same seed after a reload", 30000)
  await gridReady("Sort: Random after a reload")
  const reloaded = await firstTitles("06-movies-random-reloaded")
  ctx.check(JSON.stringify(reloaded) === JSON.stringify(shuffled), `the same seed gives the same first titles after a reload: ${reloaded.join(", ")}`)
  const apiShuffled = (await appItems(`kind=Movie&sort=random&seed=${seed}`)).items.map((item) => item.name)
  ctx.check(JSON.stringify(shuffled) === JSON.stringify(apiShuffled.slice(0, shuffled.length)), "the grid shows /api/items' order for that seed")
  await ctx.press({ role: "button", name: "Shuffle again" })
  const reseeded = await ctx.until(async () => {
    const next = new URLSearchParams((await ctx.pathname()).split("?")[1]).get("seed")
    return next && next !== seed ? next : null
  }, "Shuffle again to change the seed")
  const apiReshuffled = (await appItems(`kind=Movie&sort=random&seed=${reseeded}`)).items.map((item) => item.name)
  ctx.check(JSON.stringify(apiReshuffled) !== JSON.stringify(apiShuffled), "a new seed gives a new order")
  ctx.check(JSON.stringify([...apiReshuffled].sort()) === JSON.stringify([...apiShuffled].sort()), "a shuffle keeps exactly the same titles")
  report.random = { seed, shuffled, reloaded, reseeded, apiShuffled, apiReshuffled }
  await ctx.choose({ name: "Sort by" }, "Sort: Name")
  await gridReady("Sort: Name")

  await openFilters()
  const movieMenu = await menuNames()
  ctx.check(!movieMenu.some((name) => name.startsWith("Series status")), "Movies offer no Series status filter")
  const watchOptions = await openSubmenu("Watch status", "07-watch-status")
  ctx.check(JSON.stringify(watchOptions) === JSON.stringify(["Any watch status", "Unwatched", "In progress", "Watched"]), `Watch status options: ${watchOptions.join(", ")}`)
  await ctx.press({ role: "menuitemradio", name: "In progress" })
  await ctx.until(async () => (await ctx.pathname()).includes("watched=progress"), "watched=progress in the URL")
  await ctx.find({ role: "button", name: "Remove In progress filter" })
  await agree("Movies In progress", async () => {
    const movies = await jellyfin("/Items", { Recursive: true, IncludeItemTypes: "Movie", EnableUserData: true })
    const resumable = await serverCount({ IncludeItemTypes: "Movie", Filters: "IsResumable" })
    const app = (await appItems("kind=Movie&watched=progress")).total
    const ui = await gridReady("In progress")
    const own = movies.Items.filter((item) => item.UserData?.PlaybackPositionTicks > 0 && !item.UserData?.Played).length
    report.moviesInProgressDefinition = { jellyfinIsResumable: resumable, positionAndUnplayed: own }
    return { app: [app, ui], server: [resumable, resumable] }
  })
  await ctx.park()
  await ctx.screenshot("08-movies-in-progress")
  await ctx.press({ role: "button", name: "Remove In progress filter" })
  await ctx.until(async () => !(await ctx.pathname()).includes("watched="), "the watch filter to go")

  await openFilters()
  const ratings = await openSubmenu("Parental rating", "09-parental")
  const appRatings = (await ctx.api("/api/official-ratings?kind=Movie")).body.officialRatings
  const serverRatings = [...new Set((await jellyfin("/Items", { Recursive: true, IncludeItemTypes: "Movie" })).Items.map((item) => item.OfficialRating).filter(Boolean))].sort()
  ctx.check(JSON.stringify([...appRatings].sort()) === JSON.stringify(serverRatings), `the rating list ${appRatings.join(", ")} is the server's set`)
  ctx.check(JSON.stringify(ratings) === JSON.stringify(["Any rating", ...appRatings]), `Parental rating options: ${ratings.join(", ")}`)
  const rating = appRatings.find((value) => value === "PG") ?? appRatings[0]
  await ctx.press({ role: "menuitemradio", name: rating })
  await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get("parental") === rating, `parental=${rating} in the URL`)
  await ctx.find({ role: "button", name: `Remove Rated: ${rating} filter` })
  await ctx.find({ role: "button", name: "Filters, 1 active" })
  await agree(`Movies rated ${rating}`, async () => ({
    app: [(await appItems(`kind=Movie&parental=${encodeURIComponent(rating)}`)).total, await gridReady(`Rated ${rating}`)],
    server: Array(2).fill(await serverCount({ IncludeItemTypes: "Movie", OfficialRatings: rating })),
  }))
  report.ratings = { app: appRatings, server: serverRatings, picked: rating }
  await ctx.park()
  await ctx.screenshot("10-movies-rated")
  await ctx.press({ role: "button", name: "Clear all" })
  await ctx.until(async () => !(await ctx.exists({ role: "group", name: "Active filters" })), "the chips to go")
  await ctx.park()
  await gridReady("Movies")
  await ctx.screenshot("11-movies-controls")

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Series" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?kind=Series", "the Series library")
  await ctx.park()
  await gridReady("Series")
  report.seriesSorts = await sortOptions()
  for (const label of ["Sort: Name", "Sort: Release date", "Sort: Date show added", "Sort: Date episode added", "Sort: Rating", "Sort: Date played", "Sort: Random"]) {
    ctx.check(report.seriesSorts.includes(label), `Series offer ${label}`)
  }
  ctx.check(!report.seriesSorts.includes("Sort: Play count") && !report.seriesSorts.includes("Sort: Runtime"), "Series do not offer Play count or Runtime")
  for (const [label, id] of [["Sort: Date episode added", "episode-added"], ["Sort: Date played", "played"], ["Sort: Name", "name"]]) {
    await ctx.choose({ name: "Sort by" }, label)
    await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get("sort") === id, `sort=${id} in the URL`)
    await gridReady(label)
  }
  report.railSeriesAsc = await checkRail("Series", "kind=Series&sort=name", "asc", "Series A–Z ascending")

  const seriesList = await jellyfin("/Items", { Recursive: true, IncludeItemTypes: "Series", EnableUserData: true })
  report.unplayed = []
  for (const series of seriesList.Items) {
    const pair = await agree(`${series.Name} unwatched episodes`, async () => {
      const server = (await jellyfin("/Items", { Ids: series.Id, EnableUserData: true })).Items[0]?.UserData?.UnplayedItemCount ?? null
      const app = (await appItems("kind=Series")).items.find((item) => item.id === series.Id)?.unplayedCount ?? null
      const badge = await ctx.app.evaluate(`(() => {
        const link = document.querySelector('a[aria-label=${JSON.stringify(`Open details for ${series.Name}`)}]')
        const node = link?.querySelector("[data-unplayed-count]")
        return node ? { count: Number(node.dataset.unplayedCount), text: node.querySelector(".sr-only")?.textContent ?? "", describedBy: link.getAttribute("aria-describedby") === node.id } : null
      })()`)
      return { app: { api: app, badge: badge?.count ?? 0 }, server: { api: server, badge: server }, badge }
    })
    if (pair.app.api > 0) {
      ctx.check(pair.badge.describedBy && pair.badge.text.includes(`${pair.app.api} unwatched episode`), `${series.Name}'s badge says "${pair.badge.text}" to screen readers and describes its link`)
    }
    report.unplayed.push({ series: series.Name, app: pair.app.api, server: pair.server.api, badge: pair.badge })
  }
  ctx.check(report.unplayed.length >= 1, `compared ${report.unplayed.length} series; the demo server has ${seriesList.TotalRecordCount}`)
  await ctx.park()
  await ctx.screenshot("12-series-controls")

  await openFilters()
  const seriesMenu = await menuNames()
  ctx.check(seriesMenu.some((name) => name.startsWith("Series status")), "Series offer the Series status filter")
  report.seriesStatus = {}
  for (const status of ["Ended", "Continuing"]) {
    if ((await ctx.count({ role: "menu" })) === 0) await openFilters()
    const statuses = await openSubmenu("Series status", `13-series-status-${status}`)
    ctx.check(JSON.stringify(statuses) === JSON.stringify(["Any status", "Continuing", "Ended"]), `Series status options: ${statuses.join(", ")}`)
    await ctx.press({ role: "menuitemradio", name: status })
    await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1]).get("seriesStatus") === status, `seriesStatus=${status} in the URL`)
    await ctx.find({ role: "button", name: `Remove ${status} filter` })
    const pair = await agree(`Series ${status}`, async () => ({
      app: [(await appItems(`kind=Series&seriesStatus=${status}`)).total, await gridReady(status)],
      server: Array(2).fill(await serverCount({ IncludeItemTypes: "Series", SeriesStatus: status })),
    }))
    report.seriesStatus[status] = pair.server[0]
  }
  await ctx.press({ role: "button", name: "Remove Continuing filter" })
  await ctx.until(async () => !(await ctx.pathname()).includes("seriesStatus="), "the status filter to go")

  await openFilters()
  await openSubmenu("Watch status", "14-series-watch-status")
  await ctx.press({ role: "menuitemradio", name: "In progress" })
  await ctx.until(async () => (await ctx.pathname()).includes("watched=progress"), "watched=progress in the URL")
  await ctx.find({ role: "button", name: "Remove In progress filter" })
  await agree("Series In progress", async () => {
    const episodes = await jellyfin("/Items", { Recursive: true, IncludeItemTypes: "Episode", EnableUserData: true })
    const own = seriesInProgressFromEpisodes(episodes.Items)
    report.seriesInProgressDefinition = { jellyfinIsResumable: await serverCount({ IncludeItemTypes: "Series", Filters: "IsResumable" }), fromEpisodes: own }
    return { app: [(await appItems("kind=Series&watched=progress")).total, await gridReady("Series In progress")], server: [own, own] }
  })
  await ctx.park()
  await ctx.screenshot("15-series-in-progress")
  await ctx.press({ role: "button", name: "Remove In progress filter" })

  await ctx.app.evaluate(`history.pushState(null, "", "/library?kind=Movie&seriesStatus=Continuing&sort=episode-added"); dispatchEvent(new PopStateEvent("popstate"))`)
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library?kind=Movie&seriesStatus"), "the hand-written Movies URL")
  await gridReady("Movies with a Series-only URL")
  ctx.check(!(await ctx.exists({ role: "group", name: "Active filters" })), "a Series status in a Movies URL is not applied")
  ctx.check((await ctx.state({ role: "combobox", name: "Sort by" })).value === "Sort: Name", "a Series-only sort in a Movies URL reads as Name")
  ctx.check((await itemCount(ctx)) === (await appItems("kind=Movie")).total, "the Movies count is unfiltered")

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Favorites" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?favorite=true", "the Favorites view")
  await ctx.park()
  report.favoritesSorts = await sortOptions()
  ctx.check(!report.favoritesSorts.some((label) => ["Sort: Play count", "Sort: Runtime", "Sort: Date episode added"].includes(label)), `Favorites offer only shared sorts: ${report.favoritesSorts.join(", ")}`)

  await ctx.writeEvidence("parity-report.json", report)
}
