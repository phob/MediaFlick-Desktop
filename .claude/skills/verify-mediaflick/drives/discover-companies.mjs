// Discover's studio and network filters through the MediaFlick Companion on
// your own server (`seerr-discovery-v5`): a studio searched and applied on
// Popular movies, a network picked on Popular series, each combined with a
// genre and cleared through its chip, then a studio link on a movie's
// Discover page and a network link on a series' Discover page.
//   just verify discover-companies --account https://jellyfin.example.com
// Read-only: it makes no Seerr request.
const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const STUDIO_QUERY = "Marvel Studios"
const NETWORK = "Netflix"

// A card links to /discover/<kind>/<id>, with Discover's own query on it.
const cardIds = (ctx, kind) =>
  ctx.app.evaluate(`[...new Set([...document.querySelectorAll('main a[href^="/discover/${kind}/"]')].map((a) => Number(a.getAttribute("href").split("?")[0].split("/")[3])))]`)

const params = async (ctx) => Object.fromEntries(new URL(`mediaflick-desktop://app${await ctx.pathname()}`).searchParams)

async function waitCards(ctx, kind, description) {
  return ctx.until(async () => {
    const ids = await cardIds(ctx, kind)
    const busy = await ctx.app.evaluate(`Boolean(document.querySelector('[aria-label="Loading more titles"]'))`)
    return ids.length && !busy ? ids : null
  }, description, 30000)
}

// The wall must be exactly Seerr's first page for the same filters, and its
// first titles must name the company in their own Discover details.
async function proveWall(ctx, { kind, row, query, refKey, companyId, label }) {
  const page = (await ctx.api(`/api/seerr/discover/${row}?page=1&${query}`)).body
  const expected = page.results.map((result) => result.tmdbId)
  let shown = []
  const matched = await ctx.until(async () => {
    shown = await cardIds(ctx, kind)
    return JSON.stringify(shown.slice(0, expected.length)) === JSON.stringify(expected)
  }, `${label}: the wall to match Seerr's first page`, 15000).catch(() => false)
  ctx.check(matched, `${label}: the wall shows Seerr's first page (${expected.length} titles of ${page.totalResults}); shown ${JSON.stringify(shown.slice(0, 24))}, expected ${JSON.stringify(expected)}`)
  const sampled = []
  for (const id of expected.slice(0, 3)) {
    const detail = (await ctx.api(`/api/seerr/media/${kind}/${id}`)).body
    const refs = detail[refKey] ?? []
    sampled.push({ tmdbId: id, title: detail.title, refs })
    ctx.check(refs.some((ref) => ref.id === companyId), `${label}: ${detail.title} lists company ${companyId} in ${refKey}`)
  }
  return { totalResults: page.totalResults, shown: shown.length, sampled }
}

async function pickGenre(ctx, preferred) {
  const genres = await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll('section[aria-labelledby="discover-genres"] button[aria-pressed]')].map((b) => b.textContent.trim()).filter((name) => name && name !== "All genres")`).then((list) => (list.length ? list : null)), "the genre strip", 30000)
  const genre = genres.includes(preferred) ? preferred : genres[0]
  await ctx.press({ role: "button", name: genre })
  await ctx.until(async () => (await ctx.state({ role: "button", name: genre })).pressed === true && Boolean((await params(ctx)).genre), `${genre} pressed and genre= in the URL`)
  return { name: genre, id: Number((await params(ctx)).genre) }
}

export default async function discoverCompanies(ctx) {
  await ctx.signIn({})
  const companion = (await ctx.until(async () => (await ctx.api("/api/companion/info")).body?.info ?? null, "the Companion probe", 30000))
  ctx.check(companion.capabilities.includes("seerr-discovery-v5"), `Companion ${companion.pluginVersion} advertises seerr-discovery-v5`)
  const evidence = { companion: { version: companion.pluginVersion, capabilities: companion.capabilities } }

  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name: "Discover" })
  await ctx.until(async () => (await ctx.pathname()) === "/discover", "/discover")

  // Studio: search, apply, combine with a genre, clear through the chip.
  await ctx.press({ role: "tab", name: "Popular movies" })
  await ctx.until(async () => (await params(ctx)).row === "movies", "row=movies")
  await waitCards(ctx, "movie", "Popular movies")
  ctx.check(!(await ctx.exists({ role: "combobox", name: "Filter by network" })), "Popular movies offers no network filter")
  await ctx.press({ role: "combobox", name: "Filter by studio" })
  await ctx.fill({ role: "combobox", name: "Search studios" }, STUDIO_QUERY)
  await ctx.find({ role: "option", name: STUDIO_QUERY }, { timeout: 30000, nth: 0 })
  await ctx.snapshot("01-studio-search")
  await ctx.screenshot("01-studio-search")
  await ctx.press({ role: "option", name: STUDIO_QUERY }, { nth: 0 })
  const studioParams = await ctx.until(async () => { const p = await params(ctx); return p.studio ? p : null }, "studio= in the URL")
  const studio = { id: Number(studioParams.studio), name: studioParams.studioName }
  ctx.check(studio.name === STUDIO_QUERY, `the URL names the studio ${studio.name} (${studio.id})`)
  await ctx.find({ role: "button", name: `Clear studio filter: ${studio.name}` })
  evidence.studio = { ...studio, wall: await proveWall(ctx, { kind: "movie", row: "movies", query: `sort=popular&studio=${studio.id}`, refKey: "studioRefs", companyId: studio.id, label: `studio ${studio.name}` }) }
  await ctx.park()
  await ctx.screenshot("02-studio-applied")

  const movieGenre = await pickGenre(ctx, "Action")
  const combined = await params(ctx)
  ctx.check(combined.studio === String(studio.id), `${movieGenre.name} keeps the studio filter`)
  evidence.studio.withGenre = { genre: movieGenre, wall: await proveWall(ctx, { kind: "movie", row: "movies", query: `sort=popular&genre=${movieGenre.id}&studio=${studio.id}`, refKey: "studioRefs", companyId: studio.id, label: `studio ${studio.name} + ${movieGenre.name}` }) }
  await ctx.screenshot("03-studio-genre")

  await ctx.press({ role: "button", name: `Clear studio filter: ${studio.name}` })
  await ctx.until(async () => { const p = await params(ctx); return !p.studio && !p.studioName && p.genre === String(movieGenre.id) }, "the chip to clear the studio and keep the genre")
  ctx.check(!(await ctx.exists({ role: "button", name: `Clear studio filter: ${studio.name}` })), "the studio chip is gone")
  const afterClear = await waitCards(ctx, "movie", "the genre-only wall")
  const genreOnly = (await ctx.api(`/api/seerr/discover/movies?page=1&sort=popular&genre=${movieGenre.id}`)).body.results.map((result) => result.tmdbId)
  ctx.check(JSON.stringify(afterClear.slice(0, genreOnly.length)) === JSON.stringify(genreOnly), "after clearing, the wall is the genre-only first page")
  await ctx.screenshot("04-studio-cleared")

  // Network: pick from the Companion's list, combine with a genre, clear.
  await ctx.press({ role: "tab", name: "Popular series" })
  await ctx.until(async () => (await params(ctx)).row === "tv", "row=tv")
  await waitCards(ctx, "tv", "Popular series")
  ctx.check(!(await ctx.exists({ role: "combobox", name: "Filter by studio" })), "Popular series offers no studio filter")
  const networks = (await ctx.api("/api/seerr/networks")).body.networks
  evidence.networkList = networks
  const network = networks.find((entry) => entry.name === NETWORK)
  ctx.check(Boolean(network), `the Companion lists ${NETWORK} (${network?.id}) among ${networks.length} networks`)
  await ctx.choose({ name: "Filter by network" }, NETWORK)
  await ctx.until(async () => (await params(ctx)).network === String(network.id), `network=${network.id} in the URL`)
  await ctx.find({ role: "button", name: `Clear network filter: ${NETWORK}` })
  evidence.network = { ...network, wall: await proveWall(ctx, { kind: "tv", row: "tv", query: `sort=popular&network=${network.id}`, refKey: "networkRefs", companyId: network.id, label: `network ${NETWORK}` }) }
  await ctx.park()
  await ctx.snapshot("05-network-applied")
  await ctx.screenshot("05-network-applied")

  const tvGenre = await pickGenre(ctx, "Drama")
  ctx.check((await params(ctx)).network === String(network.id), `${tvGenre.name} keeps the network filter`)
  evidence.network.withGenre = { genre: tvGenre, wall: await proveWall(ctx, { kind: "tv", row: "tv", query: `sort=popular&genre=${tvGenre.id}&network=${network.id}`, refKey: "networkRefs", companyId: network.id, label: `network ${NETWORK} + ${tvGenre.name}` }) }
  await ctx.screenshot("06-network-genre")
  await ctx.press({ role: "button", name: `Clear network filter: ${NETWORK}` })
  await ctx.until(async () => { const p = await params(ctx); return !p.network && !p.networkName && p.genre === String(tvGenre.id) }, "the chip to clear the network and keep the genre")
  await ctx.screenshot("07-network-cleared")

  // Detail links: a movie's studio and a series' network open Discover filtered.
  await ctx.press({ role: "tab", name: "Popular movies" })
  const movies = await waitCards(ctx, "movie", "Popular movies again")
  let movieId = null
  for (const id of movies) {
    if (((await ctx.api(`/api/seerr/media/movie/${id}`)).body?.studioRefs ?? []).length) { movieId = id; break }
  }
  if (!movieId) throw new Error("no movie on Popular movies lists a studio")
  await ctx.press({ css: `main a[href^="/discover/movie/${movieId}"]` }, { nth: 0 })
  const studioLink = await ctx.until(() => ctx.app.evaluate(`document.querySelector('main a[aria-label^="Discover films from "]')?.getAttribute("aria-label") ?? null`), "a studio link on the movie page", 30000)
  await ctx.app.evaluate(`document.querySelector('main a[aria-label^="Discover films from "]').scrollIntoView({ block: "center" })`)
  await ctx.screenshot("08-movie-detail-studios")
  await ctx.press({ role: "link", name: studioLink }, { nth: 0 })
  const linked = await ctx.until(async () => { const p = await params(ctx); return (await ctx.pathname()).startsWith("/discover?") && p.studio ? p : null }, "Discover opened with a studio")
  ctx.check(linked.row === "movies" && `Discover films from ${linked.studioName}` === studioLink, `the studio link opened Popular movies filtered to ${linked.studioName} (${linked.studio})`)
  await ctx.find({ role: "button", name: `Clear studio filter: ${linked.studioName}` })
  evidence.studioLink = { fromMovie: movieId, studio: { id: Number(linked.studio), name: linked.studioName }, wall: await proveWall(ctx, { kind: "movie", row: "movies", query: `sort=popular&studio=${linked.studio}`, refKey: "studioRefs", companyId: Number(linked.studio), label: `linked studio ${linked.studioName}` }) }
  await ctx.park()
  await ctx.screenshot("09-studio-link-applied")

  await ctx.press({ role: "tab", name: "Popular series" })
  const series = await waitCards(ctx, "tv", "Popular series again")
  await ctx.press({ css: `main a[href^="/discover/tv/${series[0]}"]` }, { nth: 0 })
  const networkLink = await ctx.until(() => ctx.app.evaluate(`document.querySelector('main a[aria-label^="Discover series on "]')?.getAttribute("aria-label") ?? null`), "a network link on the series page", 30000)
  await ctx.app.evaluate(`document.querySelector('main a[aria-label^="Discover series on "]').scrollIntoView({ block: "center" })`)
  await ctx.screenshot("10-series-detail-networks")
  await ctx.press({ role: "link", name: networkLink }, { nth: 0 })
  const linkedNetwork = await ctx.until(async () => { const p = await params(ctx); return (await ctx.pathname()).startsWith("/discover?") && p.network ? p : null }, "Discover opened with a network")
  ctx.check(linkedNetwork.row === "tv" && `Discover series on ${linkedNetwork.networkName}` === networkLink, `the network link opened Popular series filtered to ${linkedNetwork.networkName} (${linkedNetwork.network})`)
  await ctx.find({ role: "button", name: `Clear network filter: ${linkedNetwork.networkName}` })
  evidence.networkLink = { fromSeries: series[0], network: { id: Number(linkedNetwork.network), name: linkedNetwork.networkName }, wall: await proveWall(ctx, { kind: "tv", row: "tv", query: `sort=popular&network=${linkedNetwork.network}`, refKey: "networkRefs", companyId: Number(linkedNetwork.network), label: `linked network ${linkedNetwork.networkName}` }) }
  await ctx.park()
  await ctx.snapshot("11-network-link-applied")
  await ctx.screenshot("11-network-link-applied")

  ctx.writeEvidence("discover-companies.json", evidence)
}
