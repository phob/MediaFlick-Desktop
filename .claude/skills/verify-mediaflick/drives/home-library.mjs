// Home and the library against Jellyfin's public demo server, read-only:
// billboard ticks, shelf arrows, a card from Home, Movies sort, a decade
// filter and its chip, an empty search, and Favorites (titled My List). Never
// presses Play/Resume or a My List toggle: the demo account is shared.
//   just verify home-library
const DEMO = "https://demo.jellyfin.org/stable"
const CARD = { css: 'a[aria-label^="Open details for"]' }
const billboardTitle = (ctx) => ctx.app.evaluate(`document.getElementById("billboard-title")?.textContent.trim() ?? null`)
const pageText = (ctx) => ctx.app.evaluate("document.body.innerText")
// The library count is an aria-live span reading "N items" or "1 item".
const itemCount = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll('[aria-live="polite"]')].map((e) => e.textContent.trim()).find((t) => /\\bitems?$/.test(t)) ?? null`)

export default async function homeLibrary(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.until(async () => (await ctx.count(CARD)) > 0, "poster cards on Home", 60000)
  await ctx.park()
  const home = await ctx.snapshot("01-home")
  await ctx.screenshot("01-home")
  ctx.step(`shelves: ${JSON.stringify(await ctx.app.evaluate(`[...document.querySelectorAll("section[aria-labelledby] h2")].map((h) => h.textContent.trim())`))}`)
  ctx.check(await ctx.exists({ role: "link", name: "MediaFlick" }), "the brand link is named on Home (sidebar pinned open)")

  // Billboard: read the toggles, only press the slide ticks.
  const first = await billboardTitle(ctx)
  if (first) {
    await ctx.find({ role: "link", name: "Details" })
    ctx.check((await ctx.exists({ role: "button", name: "Add to My List" })) || (await ctx.exists({ role: "button", name: "Remove from My List" })), "the billboard My List toggle")
    const other = [...home.matchAll(/button 'Show ([^']+)'/g)].map((match) => match[1]).find((title) => title !== first)
    if (other) {
      await ctx.press({ role: "button", name: `Show ${other}` })
      await ctx.until(async () => (await billboardTitle(ctx)) === other, `the billboard to show ${other}`)
      await ctx.screenshot("02-billboard")
    }
  }

  // Shelf arrows: Previous is disabled at the start and enables after Next.
  const shelf = [...home.matchAll(/button 'Next ([^']+)'(.*)$/gm)].find((match) => !/disabled/.test(match[2]))?.[1]
  if (shelf) {
    ctx.check((await ctx.state({ role: "button", name: `Previous ${shelf}` })).disabled === true, `Previous ${shelf} is disabled at the start`)
    await ctx.hover({ role: "button", name: `Next ${shelf}` })
    await ctx.press({ role: "button", name: `Next ${shelf}` })
    await ctx.until(async () => (await ctx.state({ role: "button", name: `Previous ${shelf}` })).disabled !== true, `Previous ${shelf} to enable`)
    await ctx.park()
    await ctx.screenshot("03-shelf-scrolled")
  } else {
    ctx.step("no shelf overflows; arrows not exercised")
  }

  // A poster card on Home opens the detail page.
  await ctx.park()
  const card = await ctx.app.evaluate(`document.querySelector('main a[aria-label^="Open details for"]')?.getAttribute("aria-label")`)
  await ctx.press({ role: "link", name: card }, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `${card} from Home`)

  // Movies: sort, filter, chip.
  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?kind=Movie", "the Movies library")
  await ctx.find({ role: "heading", name: "Movies" })
  const all = await ctx.until(() => itemCount(ctx), "the item count", 30000)
  await ctx.find({ role: "button", name: "Filters" })
  await ctx.choose({ name: "Sort by" }, "Sort: Year")
  await ctx.until(async () => /[?&]sort=year\b/.test(await ctx.pathname()), "sort=year in the URL")
  await ctx.press({ role: "button", name: "Filters" })
  // Radix names the menu after its trigger, not its aria-label.
  await ctx.find({ role: "menu", name: "Filters" })
  const menu = await ctx.snapshot("04-filters-menu")
  // Sub-menu triggers carry their current value ("Release decade Any").
  const decade = menu.match(/menuitem '(Release decade[^']*)'/)?.[1]
  ctx.check(Boolean(decade), "the Release decade sub-menu")
  await ctx.press({ role: "menuitem", name: decade })
  await ctx.press({ role: "menuitemradio", name: "1960s" })
  await ctx.until(async () => (await ctx.pathname()).includes("decade=1960"), "decade=1960 in the URL")
  await ctx.key("Escape", { code: "Escape", keyCode: 27 })
  await ctx.find({ role: "button", name: "Filters, 1 active" })
  await ctx.find({ role: "group", name: "Active filters" })
  const filtered = await ctx.until(async () => {
    const count = await itemCount(ctx)
    return count && count !== all ? count : null
  }, "the count to change")
  ctx.step(`items: ${all} → ${filtered}`)
  await ctx.screenshot("05-filtered")
  await ctx.press({ role: "button", name: "Remove Released: 1960s filter" })
  await ctx.until(async () => !(await ctx.pathname()).includes("decade="), "the decade filter to go")
  ctx.check((await ctx.pathname()).includes("sort=year"), "the sort survives removing the chip")
  ctx.check(!(await ctx.exists({ role: "group", name: "Active filters" })), "no chips left")

  // A search with no match.
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, "zzqqxx")
  await ctx.until(async () => (await ctx.pathname()) === "/library?search=zzqqxx", "the search route")
  await ctx.until(async () => (await pageText(ctx)).includes("Nothing to show"), "Nothing to show")
  ctx.check((await pageText(ctx)).includes("Nothing matches “zzqqxx”."), "the empty reason names the term")
  await ctx.screenshot("06-empty-search")

  // Favorites is the My List view: a filter, so it shows as a chip.
  await ctx.park()
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Favorites" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?favorite=true", "the Favorites route")
  await ctx.find({ role: "heading", name: "My List" })
  await ctx.find({ role: "button", name: "Filters, 1 active" })
  await ctx.find({ role: "button", name: "Remove In My List filter" })
  await ctx.park()
  await ctx.screenshot("07-favorites")
}
