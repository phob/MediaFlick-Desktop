// Detail pages against Jellyfin's public demo server, read-only: a movie's
// extras and its More info menu (opened, never used), a series' seasons and
// episode list, an episode page and its Season breadcrumb. Never presses Play,
// Resume, From start, an episode's Play, favorites, watched or track selects.
//   just verify item-detail
const DEMO = "https://demo.jellyfin.org/stable"
const MOVIE = "Night of the Living Dead"
const SERIES = "Pioneer One"
const pageText = (ctx) => ctx.app.evaluate("document.body.innerText")

// Sidebar search to a title. Off Home the sidebar is collapsed until hovered.
async function open(ctx, term, title) {
  await ctx.park()
  if ((await ctx.pathname()) !== "/") await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, term)
  await ctx.until(async () => (await ctx.pathname()) === `/library?search=${encodeURIComponent(term)}`, "the search route")
  await ctx.press({ role: "link", name: `Open details for ${title}` }, { timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `the ${title} page`)
}

export default async function itemDetail(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")

  await open(ctx, "night of the living", MOVIE)
  await ctx.find({ role: "heading", name: MOVIE })
  await ctx.find({ role: "combobox", name: "Streaming quality" })
  ctx.check((await ctx.exists({ role: "button", name: "Mark watched" })) || (await ctx.exists({ role: "button", name: "Watched" })), "the watched toggle")
  const movie = await ctx.snapshot("01-movie")
  ctx.step(`headings: ${JSON.stringify([...movie.matchAll(/heading '([^']+)'/g)].map((match) => match[1]))}`)
  ctx.step(`cast links: ${[...movie.matchAll(/link 'Find titles featuring /g)].length}`)
  await ctx.screenshot("01-movie")
  // Opening the menu is safe; its items open the user's browser.
  await ctx.press({ role: "button", name: "More info" })
  const menu = await ctx.snapshot("02-more-info")
  ctx.step(`More info: ${JSON.stringify([...menu.matchAll(/menuitem '([^']+)'/g)].map((match) => match[1]))}`)
  await ctx.key("Escape", { code: "Escape", keyCode: 27 })
  await ctx.until(async () => !(await ctx.exists({ role: "menu" })), "the menu to close")

  await open(ctx, SERIES, SERIES)
  await ctx.find({ role: "heading", name: SERIES })
  await ctx.find({ role: "list", name: "Seasons" }, { timeout: 30000 })
  await ctx.find({ role: "heading", name: "Episodes" })
  ctx.check(!(await ctx.exists({ role: "combobox", name: "Streaming quality" })), "no quality picker on a series")
  // Episode title links are named "<n>.<name>", with " (Next up)" on that one.
  const series = await ctx.until(async () => {
    const tree = await ctx.snapshot("03-series")
    return /link '\d+\./.test(tree) ? tree : null
  }, "the episode list")
  const seasons = [...series.matchAll(/button '(Season \d+|Specials)'(.*)$/gm)].map((match) => ({ name: match[1], pressed: /pressed/.test(match[2]) }))
  ctx.step(`seasons: ${JSON.stringify(seasons)}`)
  const other = seasons.find((season) => !season.pressed)
  if (other) {
    await ctx.press({ role: "button", name: other.name })
    await ctx.until(async () => (await ctx.state({ role: "button", name: other.name })).pressed === true, `${other.name} to be selected`)
    await ctx.screenshot("04-other-season")
  } else {
    ctx.step("one season only; season switching not exercised")
  }
  await ctx.screenshot("03-series")

  const episodes = [...(await ctx.snapshot("05-episodes")).matchAll(/link '(\d+\.[^']+)'/g)].map((match) => match[1])
  ctx.step(`episode links: ${JSON.stringify(episodes.slice(0, 3))}`)
  const seriesPath = await ctx.pathname()
  await ctx.park()
  await ctx.press({ role: "link", name: episodes[0] })
  await ctx.until(async () => (await ctx.pathname()) !== seriesPath && (await ctx.pathname()).startsWith("/item/"), "the episode page")
  await ctx.find({ role: "combobox", name: "Streaming quality" })
  await ctx.find({ role: "link", name: SERIES })
  const episode = await ctx.snapshot("06-episode")
  ctx.check(/S\d+E\d+/.test(await pageText(ctx)), "the episode page shows its SxEy code")
  await ctx.screenshot("06-episode")
  // Seasons are not pages: the breadcrumb lands on the series with ?season=.
  const crumb = episode.match(/link '(Season \d+)'/)?.[1]
  ctx.check(Boolean(crumb), "the Season breadcrumb")
  await ctx.press({ role: "link", name: crumb })
  await ctx.until(async () => (await ctx.pathname()).includes("?season="), "the series page with ?season=")
  await ctx.find({ role: "heading", name: SERIES })
  await ctx.screenshot("07-season-breadcrumb")
}
