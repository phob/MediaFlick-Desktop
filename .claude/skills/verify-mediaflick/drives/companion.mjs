// The MediaFlick Companion on your own server: the status page, Releases
// (Agenda, Month, month navigation, release-type toggles), Home's Releases
// shelf, and MDBList ratings on cards and a detail page after picking rating
// sources in Appearance. Read-only on the server; the Appearance change is a
// local, disposable-profile write and is restored.
//   just verify companion --account https://jellyfin.example.com
const status = async (ctx) => (await ctx.api("/api/status")).body
const articles = (ctx) => ctx.app.evaluate(`document.querySelectorAll("main article").length`)
const monthTitle = (ctx) => ctx.app.evaluate(`document.querySelector("main h1")?.textContent.trim() ?? null`)
const monthName = (date) => date.toLocaleDateString(undefined, { month: "long", year: "numeric" })
const SIDEBAR = { css: '[data-sidebar="sidebar"]' }

async function sidebarLink(ctx, name) {
  await ctx.hover(SIDEBAR)
  await ctx.press({ role: "link", name })
}

export default async function companion(ctx) {
  await ctx.signIn({})
  await ctx.until(async () => {
    const now = await status(ctx)
    return now?.bootstrap?.state === "ready" && now.syncProgress?.active === false
  }, "the first sync to finish", 300000)
  const evidence = {}

  const info = (await ctx.api("/api/companion/info")).body
  evidence.companion = { available: info.available, compatible: info.compatible, pluginVersion: info.info?.pluginVersion, capabilities: info.info?.capabilities, services: info.info?.services }
  ctx.check(info.available && info.compatible, `the Companion ${info.info?.pluginVersion} is available and compatible`)
  ctx.check(["calendar", "ratings-v1", "seerr"].every((capability) => info.info.capabilities.includes(capability)), "it advertises calendar, ratings-v1 and seerr")
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "MediaFlick Companion" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/integrations/companion", "the Companion page")
  for (const heading of ["Connection", "Desktop features", "Seerr", "Sonarr", "Radarr", "MDBList", "TMDB"]) await ctx.find({ role: "heading", name: heading })
  const statusTexts = await ctx.app.evaluate(`[...document.querySelectorAll("main h3")].map((h) => [h.textContent.trim(), h.closest("div")?.parentElement?.innerText.match(/\\b(available|unavailable|compatible|missing)\\b/i)?.[0]?.toLowerCase() ?? null])`)
  evidence.statusPage = Object.fromEntries(statusTexts)
  ctx.check(statusTexts.every(([, state]) => state === "available" || state === "compatible"), `every row reads available or compatible (${JSON.stringify(evidence.statusPage)})`)
  await ctx.park()
  const activeNav = await ctx.app.evaluate(`[...document.querySelectorAll('nav[aria-label="Settings navigation"] a[data-active="true"]')].map((a) => a.textContent.trim())`)
  ctx.check(activeNav.length === 1 && activeNav[0] === "MediaFlick Companion", `only MediaFlick Companion is active in Settings navigation (${activeNav.join(", ")})`)
  await ctx.snapshot("01-companion-status")
  await ctx.screenshot("01-companion-status")

  await sidebarLink(ctx, "Releases")
  await ctx.until(async () => (await ctx.pathname()).startsWith("/calendar"), "/calendar")
  const now = new Date()
  await ctx.until(async () => (await monthTitle(ctx)) === monthName(now), `the ${monthName(now)} heading`)
  ctx.check((await ctx.state({ role: "tab", name: "Agenda" })).selected === true, "Agenda is the default tab")
  const agenda = await ctx.until(async () => (await articles(ctx)) || null, "release entries in the agenda", 30000)
  evidence.releases = { agendaEntries: agenda }
  await ctx.snapshot("02-releases-agenda")
  await ctx.screenshot("02-releases-agenda")
  await ctx.press({ role: "button", name: "Episodes" })
  const withoutEpisodes = await ctx.until(async () => {
    const count = await articles(ctx)
    return count !== agenda ? { count } : null
  }, "the Episodes toggle to change the agenda", 10000)
  await ctx.press({ role: "button", name: "Episodes" })
  await ctx.until(async () => (await articles(ctx)) === agenda, "the agenda restored with Episodes on")
  evidence.releases.withoutEpisodes = withoutEpisodes.count
  ctx.check(withoutEpisodes.count < agenda, `Episodes off hides episode entries (${agenda} → ${withoutEpisodes.count})`)
  await ctx.press({ role: "tab", name: "Month" })
  const cells = await ctx.until(async () => (await ctx.app.evaluate(`document.querySelectorAll("[data-calendar-cell]").length`)) || null, "the month grid")
  ctx.check(cells === 42, `the month grid has 42 day cells (${cells})`)
  await ctx.screenshot("03-releases-month")
  await ctx.press({ role: "button", name: "Next month" })
  const next = new Date(now.getFullYear(), now.getMonth() + 1, 1)
  await ctx.until(async () => (await monthTitle(ctx)) === monthName(next), `the ${monthName(next)} heading`)
  await ctx.press({ role: "button", name: "Today" })
  await ctx.until(async () => (await monthTitle(ctx)) === monthName(now), "Today returns to the current month")

  await sidebarLink(ctx, "Home")
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  const shelf = await ctx.until(() => ctx.app.evaluate(`(() => {
    const section = [...document.querySelectorAll("section[aria-labelledby]")].find((s) => s.querySelector("h2")?.textContent.trim() === "Releases")
    if (!section) return null
    return {
      cards: [...section.querySelectorAll("a[aria-label^='Open ']")].map((a) => a.getAttribute("aria-label")),
      today: !!section.querySelector("[role='separator'][aria-label='Today']"),
      all: section.querySelector("a[href='/calendar']")?.textContent.trim() ?? null,
    }
  })()`), "the Releases shelf on Home", 30000)
  evidence.releasesShelf = { cards: shelf.cards.length, today: shelf.today, first: shelf.cards.slice(0, 3) }
  ctx.check(shelf.cards.length > 0 && shelf.cards.length <= 24, `the shelf holds 1–24 release cards (${shelf.cards.length})`)
  ctx.check(shelf.all === "All", "the shelf links All to /calendar")
  await ctx.app.evaluate(`[...document.querySelectorAll("section[aria-labelledby]")].find((s) => s.querySelector("h2")?.textContent.trim() === "Releases")?.scrollIntoView({ block: "center" })`)
  await ctx.park()
  await ctx.screenshot("04-home-releases")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Appearance" })
  const imdb = await ctx.find({ role: "checkbox", name: "IMDb" }, { timeout: 20000 })
  ctx.check(!imdb.properties.disabled, "the IMDb rating source can be picked")
  await ctx.press({ role: "checkbox", name: "IMDb" })
  await ctx.press({ role: "checkbox", name: "Rotten Tomatoes Critics" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => {
    const sources = (await ctx.api("/api/settings")).body?.appearance?.ratingSources ?? []
    return sources.includes("imdb") && sources.length === 2
  }, "the saved rating sources")
  await sidebarLink(ctx, "Movies")
  const rated = await ctx.until(() => ctx.app.evaluate(`(() => {
    const lists = [...document.querySelectorAll('dl[aria-label^="Ratings for "]')]
    return lists.length ? { cards: lists.length, sample: lists.slice(0, 2).map((dl) => [dl.getAttribute("aria-label"), [...dl.querySelectorAll("[aria-label]")].map((n) => n.getAttribute("aria-label"))]) } : null
  })()`), "rating readouts on movie cards", 45000)
  evidence.ratings = rated
  ctx.check(rated.sample.every(([, values]) => values.every((value) => /^(IMDb|Rotten Tomatoes Critics) rating /.test(value))), `card readouts show only the picked sources (${JSON.stringify(rated.sample[0])})`)
  await ctx.park()
  await ctx.screenshot("05-movie-ratings")
  const firstCard = await ctx.app.evaluate(`document.querySelector('a[aria-label^="Open details for"]')?.getAttribute("aria-label")`)
  await ctx.press({ role: "link", name: firstCard }, { nth: 0 })
  const detail = await ctx.until(() => ctx.app.evaluate(`document.querySelector('main dl[aria-label^="Ratings for "]')?.getAttribute("aria-label") ?? null`), "the rating readout on the detail page", 30000)
  evidence.ratings.detail = detail
  await ctx.snapshot("06-detail-ratings")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Appearance" })
  await ctx.press({ role: "button", name: "Restore defaults" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => ((await ctx.api("/api/settings")).body?.appearance?.ratingSources ?? []).length === 0, "rating sources cleared again")
  ctx.writeEvidence("companion.json", evidence)
}
