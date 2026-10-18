// Seerr rows are joined to the library by media kind. The app signs in
// through a loopback proxy that answers the Companion's Seerr search for one
// query with a crafted page: a person whose TMDB id is a library series' TMDB
// id, a series row for that series, a movie row, and a row of an unknown kind
// with the same TMDB id as the series. Desktop drops the person and the
// unknown row; the series and movie rows keep their library ids in
// `/api/seerr/search`, and Discover search shows them without an application
// error. Read-only.
//   just verify seerr-kinds --account https://jellyfin.example.com --signed-out
const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const QUERY = "mediaflick seerr kinds probe"
const SEARCH_PATH = "/MediaFlick/seerr/search"
const TITLES = {
  person: "Seerr kinds person",
  tv: "Seerr kinds series",
  movie: "Seerr kinds movie",
  unknown: "Seerr kinds unknown",
}

const row = (mediaType, tmdbId, title) => ({
  mediaType,
  tmdbId,
  title,
  year: 2001,
  overview: null,
  posterPath: null,
  backdropPath: null,
  voteAverage: null,
  status: "unknown",
  status4k: "unknown",
  libraryItemId: null,
  activity: null,
  activity4k: null,
})

// Titles whose TMDB id no other title of the same kind shares, so the cache
// maps that id to exactly one item.
function uniqueByTmdb(items) {
  const counts = new Map()
  for (const item of items) counts.set(item.tmdb, (counts.get(item.tmdb) ?? 0) + 1)
  return items.filter((item) => counts.get(item.tmdb) === 1)
}

async function cached(ctx, kind) {
  const { Items = [] } = await ctx.jellyfin(`/Items?userId=${ctx.account.userId}&recursive=true&includeItemTypes=${kind}&fields=ProviderIds&sortBy=SortName&sortOrder=Ascending`)
  const listed = uniqueByTmdb(Items
    .map((item) => ({ id: item.Id, name: item.Name, tmdb: item.ProviderIds?.Tmdb ?? item.ProviderIds?.tmdb ?? null, played: item.UserData?.Played === true }))
    .filter((item) => item.tmdb && /^\d+$/.test(item.tmdb)))
  for (const item of kind === "Movie" ? [...listed.filter((entry) => entry.played), ...listed.filter((entry) => !entry.played)] : listed) {
    const detail = (await ctx.api(`/api/item/${encodeURIComponent(item.id)}`)).body
    if (detail?.layout === "title" && detail.providerIds?.tmdb === item.tmdb) return { ...item, played: detail.played }
  }
  return null
}

const cardsIn = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll('main a[href^="/discover/"], main a[href^="/item/"]')].map((a) => ({ href: a.getAttribute("href"), text: a.innerText.replace(/\\s+/g, " ").trim() }))`)

export default async function seerrKinds(ctx) {
  if (!ctx.account?.signedOut) throw new Error("seerr-kinds needs --account SERVER --signed-out: the app signs in to the proxy with its own token")
  const report = { query: QUERY, upstreamPath: SEARCH_PATH, checks: [] }
  const expect = (condition, message, observed) => {
    report.checks.push({ ok: Boolean(condition), message, observed })
    ctx.step(`${condition ? "ok" : "FAIL"}: ${message}${observed === undefined ? "" : ` (observed ${JSON.stringify(observed)})`}`)
  }
  let page = null
  const answer = (url, method) =>
    method === "GET" && url.pathname === SEARCH_PATH && url.searchParams.get("query") === QUERY && page ? { status: 200, body: page } : null
  const proxy = await ctx.proxy({ upstream: ctx.account.server, answer })
  try {
    await ctx.fill({ role: "textbox", name: "Server" }, proxy.url)
    await ctx.quickConnect({ approve: true, timeout: 60000 })
    ctx.revokeAppSessionAfterRun()
    await ctx.until(async () => {
      const status = (await ctx.api("/api/status")).body
      return status?.bootstrap?.state === "ready" && status.syncProgress?.active === false
    }, "the first sync through the proxy to finish", 300000)
    const seerr = (await ctx.api("/api/seerr/status")).body
    ctx.check(seerr?.linked === true, `Seerr is linked through the Companion as ${seerr?.user?.name}`)

    const series = await cached(ctx, "Series")
    const movie = await cached(ctx, "Movie")
    if (!series || !movie) throw new Error(`the account needs a cached series and movie with a TMDB id no other title of its kind shares (series: ${series?.name}, movie: ${movie?.name})`)
    report.picks = { series, movie }
    ctx.step(`series ${series.name} (TMDB ${series.tmdb}), movie ${movie.name} (TMDB ${movie.tmdb}, played ${movie.played})`)

    const results = [
      row("person", Number(series.tmdb), TITLES.person),
      row("tv", Number(series.tmdb), TITLES.tv),
      row("movie", Number(movie.tmdb), TITLES.movie),
      row("collection", Number(series.tmdb), TITLES.unknown),
    ]
    page = { page: 1, totalPages: 1, totalResults: results.length, results }
    report.answeredPage = page

    const searched = await ctx.api(`/api/seerr/search?${new URLSearchParams({ q: QUERY, page: "1" })}`)
    report.apiAnswer = searched
    expect(searched.status === 200, "/api/seerr/search answers 200", searched.status)
    expect(proxy.stats.answered.some((entry) => entry.path === SEARCH_PATH), `the proxy answered the Companion's ${SEARCH_PATH}`)
    const byTitle = Object.fromEntries((searched.body?.results ?? []).map((result) => [result.title, result]))
    const person = byTitle[TITLES.person]
    const tv = byTitle[TITLES.tv]
    const film = byTitle[TITLES.movie]
    const unknown = byTitle[TITLES.unknown]
    expect(person === undefined, "the person row is dropped", person)
    expect(unknown === undefined, "the unknown-kind row is dropped", unknown)
    expect(tv && film, "the tv and movie rows come back", Object.keys(byTitle))
    expect(tv?.libraryItemId === series.id, `the tv row has the series id of ${series.name}`, tv?.libraryItemId)
    expect(film?.libraryItemId === movie.id, `the movie row has the movie id of ${movie.name}`, film?.libraryItemId)
    expect(film?.played === movie.played, `the movie row's played matches the cache (${movie.played})`, film?.played)

    await ctx.hover(SIDEBAR)
    await ctx.press({ role: "link", name: "Discover" })
    await ctx.until(async () => (await ctx.pathname()).startsWith("/discover"), "/discover")
    await ctx.fill({ role: "textbox", name: "Search Seerr" }, QUERY)
    const outcome = await ctx.until(async () => {
      if (await ctx.exists({ role: "heading", name: "Unexpected Application Error!" })) return "error"
      const found = await cardsIn(ctx)
      return [TITLES.tv, TITLES.movie].every((title) => found.some((card) => card.text.includes(title))) ? "cards" : null
    }, "the series and movie cards or an application error", 30000)
    await ctx.park()
    await ctx.snapshot("01-seerr-search")
    await ctx.screenshot("01-seerr-search")
    report.uiError = outcome === "error" ? await ctx.app.evaluate(`[...document.querySelectorAll("h1, h2, h3, pre")].map((node) => node.innerText.trim()).join("\\n")`) : null
    expect(outcome === "cards", "Discover search renders the crafted page without an application error", report.uiError)
    if (outcome === "cards") {
      const cards = await cardsIn(ctx)
      report.cards = cards
      const card = (title) => cards.find((entry) => entry.text.includes(title))
      expect(card(TITLES.person) === undefined, "Discover shows no person card", card(TITLES.person))
      expect(card(TITLES.unknown) === undefined, "Discover shows no unknown-kind card", card(TITLES.unknown))
      expect(card(TITLES.tv).text.includes("In your library"), "the series card shows In your library", card(TITLES.tv).text)
      expect(card(TITLES.movie).text.includes("In your library"), "the movie card shows In your library", card(TITLES.movie).text)
    }
    report.proxy = { forwarded: proxy.stats.forwarded, answered: proxy.stats.answered }
  } finally {
    ctx.writeEvidence("seerr-kinds.json", report)
  }

  const failed = report.checks.filter((entry) => !entry.ok).map((entry) => entry.message)
  ctx.check(failed.length === 0, `${report.checks.length - failed.length} of ${report.checks.length} checks passed${failed.length ? `; failed: ${failed.join(" | ")}` : ""}`)
}
