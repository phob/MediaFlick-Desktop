// Seerr through the MediaFlick Companion on your own server: Discover's rows,
// a genre, a search with an owned title, a title page, then a real request
// from a Discover card and its cancellation from Requests.
//   just verify discover-requests --account https://jellyfin.example.com
// Writes to the server owner's Seerr: it requests one movie nobody has
// requested and cancels it again (Seerr may notify its admin meanwhile). If
// the drive fails in between, the session cancels it after the run.
const SIDEBAR = { css: '[data-sidebar="sidebar"]' }
const OWNED = "28 Days Later"

async function sidebarLink(ctx, name) {
  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name })
}

// "20 loaded / 1,000"; CSS upper-cases it, so innerText would read LOADED.
const loadedCount = (ctx) => ctx.app.evaluate(`Number(document.querySelector("main")?.textContent.match(/(\\d+) loaded \\//i)?.[1] ?? 0)`)

// A movie Seerr does not have and nobody requested, from Popular movies.
async function pickUnrequested(ctx) {
  const requested = new Set(((await ctx.api("/api/seerr/requests?filter=all&take=100")).body?.results ?? []).map((request) => request.tmdbId))
  for (let page = 1; page <= 5; page++) {
    const { results = [] } = (await ctx.api(`/api/seerr/discover/movies?page=${page}`)).body ?? {}
    const pick = results.find((result) => result.mediaType === "movie" && result.status === "unknown" && !result.libraryItemId && !requested.has(result.tmdbId) && result.year)
    if (pick) return pick
  }
  throw new Error("no unrequested movie in the first five pages of Popular movies")
}

export default async function discoverRequests(ctx) {
  await ctx.signIn({})
  // Owned titles are matched against the local catalog, so it must be complete.
  await ctx.until(async () => {
    const now = (await ctx.api("/api/status")).body
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  const seerr = (await ctx.api("/api/seerr/status")).body
  ctx.check(seerr?.linked === true && seerr.capabilities?.movie?.request === true, `Seerr is linked as ${seerr?.user?.name} and may request movies`)
  const evidence = { seerr: { user: seerr.user?.name, autoApprove: seerr.capabilities.movie.autoApprove, partialRequests: seerr.instance?.partialRequestsEnabled } }

  // Discover: the five rows, Trending first.
  await sidebarLink(ctx, "Discover")
  await ctx.until(async () => (await ctx.pathname()) === "/discover", "/discover")
  await ctx.find({ role: "textbox", name: "Search Seerr" })
  for (const tab of ["Trending", "Popular movies", "Popular series", "Upcoming movies", "Upcoming series"]) await ctx.find({ role: "tab", name: tab })
  ctx.check((await ctx.state({ role: "tab", name: "Trending" })).selected === true, "Trending is the first row")
  await ctx.find({ role: "heading", name: "What everyone is watching" }, { timeout: 30000 })
  evidence.trendingLoaded = await ctx.until(async () => (await loadedCount(ctx)) || null, "Trending titles to load", 30000)
  await ctx.park()
  await ctx.snapshot("01-discover-trending")
  await ctx.screenshot("01-discover-trending")

  // Popular movies and one genre.
  await ctx.press({ role: "tab", name: "Popular movies" })
  await ctx.until(async () => (await ctx.pathname()).includes("row=movies"), "row=movies in the URL")
  await ctx.find({ role: "heading", name: "Popular movies" }, { timeout: 30000 })
  const genre = await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll('section[aria-labelledby="discover-genres"] button[aria-pressed]')].map((b) => b.textContent.trim()).find((name) => name && name !== "All genres") ?? null`), "the genre strip", 30000)
  await ctx.press({ role: "button", name: genre })
  await ctx.until(async () => (await ctx.state({ role: "button", name: genre })).pressed === true && (await ctx.pathname()).includes("genre="), `${genre} pressed and genre= in the URL`)
  evidence.genre = { name: genre, loaded: await ctx.until(async () => (await loadedCount(ctx)) || null, "genre titles to load", 30000) }
  await ctx.screenshot("02-discover-genre")

  // Search for a title the library owns.
  await ctx.fill({ role: "textbox", name: "Search Seerr" }, OWNED)
  await ctx.find({ role: "heading", name: `Results for “${OWNED}”` }, { timeout: 30000 })
  const ownedCard = await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll('main a[href^="/discover/movie/"]')].map((a) => a.innerText.replace(/\\s+/g, " ")).find((text) => text.includes("In your library") && text.includes(${JSON.stringify(OWNED)})) ?? null`), `an "In your library" card for ${OWNED}`, 30000)
  evidence.ownedCard = ownedCard
  await ctx.snapshot("03-search-owned")
  const ownedHref = await ctx.app.evaluate(`[...document.querySelectorAll('main a[href^="/discover/movie/"]')].find((a) => a.innerText.includes("In your library") && a.innerText.includes(${JSON.stringify(OWNED)}))?.getAttribute("href")`)
  await ctx.press({ css: `main a[href="${ownedHref}"]` })
  await ctx.find({ role: "heading", name: OWNED, level: 1 }, { timeout: 30000 }).catch(() => ctx.find({ role: "heading", name: OWNED }, { timeout: 5000, nth: 0 }))
  await ctx.find({ role: "link", name: "Open in library" }, { timeout: 30000 })
  await ctx.screenshot("04-discover-owned-detail")
  await ctx.press({ role: "link", name: "Back to discovery" })

  // Request a movie from its Discover card.
  const pick = await pickUnrequested(ctx)
  evidence.request = { tmdbId: pick.tmdbId, title: pick.title, year: pick.year }
  ctx.step(`requesting ${pick.title} (${pick.year}), TMDB ${pick.tmdbId}`)
  await ctx.fill({ role: "textbox", name: "Search Seerr" }, pick.title)
  await ctx.find({ role: "heading", name: `Results for “${pick.title}”` }, { timeout: 30000 })
  const card = { css: `main a[href^="/discover/movie/${pick.tmdbId}"]` }
  await ctx.find(card, { timeout: 30000, nth: 0 })
  const nth = await ctx.app.evaluate(`(() => {
    const buttons = [...document.querySelectorAll('button[aria-label=${JSON.stringify(`Request ${pick.title}`)}]')]
    return buttons.findIndex((b) => b.parentElement?.querySelector('a[href^="/discover/movie/${pick.tmdbId}"]'))
  })()`)
  if (nth < 0) throw new Error(`no quick Request button on the ${pick.title} card`)
  await ctx.hover(card, { nth: 0 })
  await ctx.press({ role: "button", name: `Request ${pick.title}` }, { nth })
  await ctx.find({ role: "dialog", name: `Request ${pick.title} (${pick.year})` }, { timeout: 15000 })
  await ctx.snapshot("05-request-dialog")
  await ctx.screenshot("05-request-dialog")
  await ctx.press({ role: "button", name: "Request" })
  await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll("[data-sonner-toast]")].some((t) => /^(Requested|Approved)/.test(t.textContent.trim()))`), "the Requested toast", 20000)
  const created = await ctx.until(async () => ((await ctx.api("/api/seerr/requests?filter=all&take=40")).body?.results ?? []).find((request) => request.tmdbId === pick.tmdbId) ?? null, "the new request in /api/seerr/requests", 20000)
  ctx.cancelSeerrRequestAfterRun(created.id)
  evidence.request.created = { id: created.id, status: created.status, mediaStatus: created.mediaStatus }
  ctx.check(created.status === "pending", `request ${created.id} awaits approval`)

  // Cancel it from Requests.
  await sidebarLink(ctx, "Requests")
  await ctx.until(async () => (await ctx.pathname()).startsWith("/requests"), "/requests")
  await ctx.press({ role: "tab", name: "Awaiting approval" })
  await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll("main li")].some((li) => li.innerText.includes(${JSON.stringify(pick.title)}))`), `${pick.title} in Awaiting approval`, 30000)
  await ctx.snapshot("06-requests-pending")
  await ctx.screenshot("06-requests-pending")
  const cancelIndex = await ctx.app.evaluate(`(() => {
    const buttons = [...document.querySelectorAll("main li button")].filter((b) => b.textContent.trim() === "Cancel")
    return buttons.findIndex((b) => b.closest("li").innerText.includes(${JSON.stringify(pick.title)}))
  })()`)
  await ctx.press({ role: "button", name: "Cancel" }, { nth: cancelIndex })
  await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll("[data-sonner-toast]")].some((t) => t.textContent.trim().startsWith("Request cancelled"))`), "the Request cancelled toast", 20000)
  await ctx.until(async () => !((await ctx.api("/api/seerr/requests?filter=all&take=40")).body?.results ?? []).some((request) => request.id === created.id), `request ${created.id} gone from /api/seerr/requests`, 20000)
  evidence.request.afterCancel = (await ctx.api(`/api/seerr/media/movie/${pick.tmdbId}`)).body?.status ?? null
  await ctx.sleep(1000)
  await ctx.snapshot("07-requests-after-cancel")
  ctx.writeEvidence("discover-requests.json", evidence)
}
