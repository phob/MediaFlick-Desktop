// Library views without a media kind (sidebar search, Favorites, Home's
// Favorites shelf) list only movies and series, never seasons or episodes.
// Searches typed into the sidebar for terms the demo server's episodes and
// season share (`night`, `season`, `earthfall`), then reads `/api/items` the
// way those views do: by search, by favorite state (the demo account has no
// favorites, so `favorite=false` exercises the same kindless query) and the
// A–Z letters. An explicit `kind=Episode` must still list episodes. Read-only
// against the demo server.
//   just verify top-level-titles
const TOP_LEVEL = new Set(["Movie", "Series"])
const CARD = { css: 'main a[aria-label^="Open details for"]' }

const kindsOf = (items) => [...new Set(items.map((item) => item.kind))].sort()
const nested = (items) => items.filter((item) => !TOP_LEVEL.has(item.kind)).map((item) => `${item.kind}: ${item.name}`)

export default async function topLevelTitles(ctx) {
  await ctx.signIn({ server: "https://demo.jellyfin.org/stable", username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.until(async () => ((await ctx.api("/api/items?kind=Episode&limit=1")).body?.total ?? 0) > 0, "episodes in the library cache", 60000)

  const report = { searches: {}, reads: {} }
  for (const term of ["night", "season", "earthfall"]) {
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.fill({ role: "textbox", name: "Search the library" }, term)
    await ctx.until(async () => new URLSearchParams((await ctx.pathname()).split("?")[1] ?? "").get("search") === term, `search=${term} in the URL`)
    await ctx.until(async () => {
      const text = await ctx.app.evaluate("document.querySelector('main')?.innerText ?? ''")
      return text.includes("Nothing to show") || (await ctx.count(CARD)) > 0
    }, `the results for ${term}`, 15000)
    await ctx.park()
    const cards = await ctx.app.evaluate(`[...document.querySelectorAll('main a[aria-label^="Open details for"]')].map((a) => ({ name: a.getAttribute("aria-label").replace("Open details for ", ""), text: a.innerText.replace(/\\s+/g, " ").trim() }))`)
    const api = await ctx.api(`/api/items?search=${encodeURIComponent(term)}&limit=60`)
    report.searches[term] = {
      cards,
      apiStatus: api.status,
      apiTotal: api.body?.total ?? null,
      apiKinds: kindsOf(api.body?.items ?? []),
      nested: nested(api.body?.items ?? []),
    }
    ctx.step(`search ${term}: cards ${JSON.stringify(cards.map((card) => card.name))}, api kinds ${report.searches[term].apiKinds.join(",")}`)
    await ctx.snapshot(`search-${term}`)
    await ctx.screenshot(`search-${term}`)
  }

  for (const [label, path] of [
    ["favorites", "/api/items?favorite=true&limit=200"],
    ["not favorites", "/api/items?favorite=false&limit=200"],
    ["explicit episodes", "/api/items?kind=Episode&limit=200"],
  ]) {
    const api = await ctx.api(path)
    report.reads[label] = { path, status: api.status, total: api.body?.total ?? null, kinds: kindsOf(api.body?.items ?? []), nested: nested(api.body?.items ?? []) }
    ctx.step(`${label}: ${api.status} total ${report.reads[label].total} kinds ${report.reads[label].kinds.join(",")}`)
  }
  const letters = await ctx.api("/api/items/letters?favorite=false")
  const topLevelLetters = await ctx.api("/api/items/letters?kind=Movie,Series&favorite=false")
  const titles = (await ctx.api("/api/items?kind=Movie,Series&favorite=false&limit=200")).body?.items ?? []
  report.reads.letters = { status: letters.status, kindless: letters.body?.letters ?? null, movieAndSeries: topLevelLetters.body?.letters ?? null }
  const home = await ctx.api("/api/home")
  report.reads.homeRows = (home.body?.rows ?? []).map((row) => ({ id: row.id, kinds: kindsOf(row.items) }))

  ctx.writeEvidence("top-level-titles.json", report)

  const night = report.searches.night
  ctx.check(night.cards.some((card) => card.name === "Night of the Living Dead"), "searching night still finds the movie")
  for (const [term, result] of Object.entries(report.searches)) {
    ctx.check(result.apiStatus === 200 && result.nested.length === 0, `search ${term} lists only movies and series (got ${JSON.stringify(result.nested)})`)
    ctx.check(result.cards.length === result.apiTotal, `search ${term} shows one card per title (${result.cards.length} cards, ${result.apiTotal} titles)`)
  }
  for (const label of ["favorites", "not favorites"]) {
    const read = report.reads[label]
    ctx.check(read.status === 200 && read.nested.length === 0, `a ${label} view lists only movies and series (got ${read.nested.length} nested: ${JSON.stringify(read.nested.slice(0, 3))})`)
  }
  ctx.check(report.reads["not favorites"].total === titles.length, `a kindless view counts the same titles as Movies and Series (${report.reads["not favorites"].total} vs ${titles.length})`)
  const { kindless, movieAndSeries } = report.reads.letters
  ctx.check(report.reads.letters.status === 200 && JSON.stringify(kindless) === JSON.stringify(movieAndSeries), `a kindless view's A–Z rail matches Movies and Series (${JSON.stringify(kindless)} vs ${JSON.stringify(movieAndSeries)})`)
  const episodes = report.reads["explicit episodes"]
  ctx.check(episodes.total > 0 && episodes.kinds.join(",") === "Episode", `kind=Episode still lists episodes (${episodes.total})`)
  ctx.check(report.reads.homeRows.every((row) => row.kinds.every((kind) => TOP_LEVEL.has(kind)) || row.id === "recentlyAddedShows"), "Home shelves other than Recently Added Shows hold only movies and series")
}
