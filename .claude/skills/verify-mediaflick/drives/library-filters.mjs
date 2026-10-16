// The library beyond Movies: the Shows library, Sort: Rating, the Genre and
// Watch status filters (the menu closes on a pick and is named after its
// trigger), chips and Clear all, a one-character search that never commits,
// clearing a search, and signing out from a library page. Read-only.
//   just verify library-filters
const DEMO = "https://demo.jellyfin.org/stable"
const USER_MENU = { role: "button", name: "D demo demo.jellyfin.org" }
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const LIBRARY_ROUTE = /^\/library\/[0-9a-f]+$/
const itemCount = (ctx) => ctx.app.evaluate(`(() => { const m = document.body.innerText.match(/^([\\d,]+) items?$/m); return m ? m[1] : null })()`)

export default async function libraryFilters(ctx) {
  const results = {}
  await ctx.signIn({ server: DEMO, username: "demo" })

  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => LIBRARY_ROUTE.test(await ctx.pathname()), "the Movies library")
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Shows" })
  await ctx.until(async () => LIBRARY_ROUTE.test(await ctx.pathname()), "the Shows library")
  await ctx.find({ role: "heading", name: "Shows" })
  results.seriesCount = await ctx.until(() => itemCount(ctx), "the Series count", 30000)
  await ctx.park()
  await ctx.screenshot("01-series")

  await ctx.choose({ name: "Sort by" }, "Sort: Rating")
  await ctx.until(async () => /[?&]sort=rating\b/.test(await ctx.pathname()), "sort=rating in the URL")
  await ctx.press({ role: "button", name: "Filters" })
  await ctx.find({ role: "menu", name: "Filters" })
  const menu = await ctx.snapshot("02-filters-menu")
  results.subMenus = [...menu.matchAll(/menuitem '([^']*)'/g)].map((m) => m[1])
  const genre = results.subMenus.find((name) => name.startsWith("Genre"))
  await ctx.press({ role: "menuitem", name: genre })
  const genreMenu = await ctx.snapshot("03-genre-menu")
  results.genreOptions = [...genreMenu.matchAll(/menuitemradio '([^']*)'/g)].map((m) => m[1])
  const pick = results.genreOptions.find((name) => name !== "Any genre" && !name.startsWith("Loading"))
  await ctx.press({ role: "menuitemradio", name: pick })
  await ctx.until(async () => (await ctx.pathname()).includes("genre="), "genre= in the URL")
  await ctx.until(async () => (await ctx.count({ role: "menu" })) === 0, "the menu to close after a pick", 3000)
  await ctx.find({ role: "button", name: `Remove Genre: ${pick} filter` })
  await ctx.press({ role: "button", name: "Filters, 1 active" })
  // Radix names the menu after its trigger, so the name follows the count.
  await ctx.find({ role: "menu", name: "Filters, 1 active" })
  const reopened = await ctx.snapshot("04-filters-reopened")
  const watch = [...reopened.matchAll(/menuitem '(Watch status[^']*)'/g)].map((m) => m[1])[0]
  await ctx.press({ role: "menuitem", name: watch })
  await ctx.press({ role: "menuitemradio", name: "Unwatched" })
  await ctx.until(async () => (await ctx.pathname()).includes("watched=unwatched"), "watched=unwatched in the URL")
  await ctx.find({ role: "button", name: "Filters, 2 active" })
  await ctx.find({ role: "button", name: "Remove Unwatched filter" })
  await ctx.park()
  await ctx.screenshot("05-two-filters")
  await ctx.press({ role: "button", name: "Remove Unwatched filter" })
  await ctx.until(async () => !(await ctx.pathname()).includes("watched="), "the watched filter to go")
  ctx.check((await ctx.pathname()).includes("genre="), "the genre filter stays")
  await ctx.press({ role: "button", name: "Clear all" })
  await ctx.until(async () => !(await ctx.exists({ role: "group", name: "Active filters" })), "the chips to go")
  ctx.check((await ctx.pathname()).includes("sort=rating"), "Clear all keeps the sort")

  const before = await ctx.pathname()
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, "n")
  await sleep(600)
  ctx.check((await ctx.pathname()) === before, "a one-character term does not navigate")
  await ctx.fill({ role: "textbox", name: "Search the library" }, "night of the living")
  await ctx.until(async () => (await ctx.pathname()) === "/library?search=night%20of%20the%20living", "the search route")
  await ctx.fill({ role: "textbox", name: "Search the library" }, "")
  await ctx.until(async () => (await ctx.pathname()) === "/library", "clearing the search to go to /library")
  await ctx.find({ role: "heading", name: "Library" })

  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => LIBRARY_ROUTE.test(await ctx.pathname()), "the Movies library")
  const moviesRoute = await ctx.pathname()
  await ctx.park()
  await ctx.hover(USER_MENU)
  await ctx.press(USER_MENU)
  await ctx.press({ role: "menuitem", name: "Sign out" })
  await ctx.until(async () => (await ctx.find({ role: "textbox", name: "Server" })).properties.value, "the remembered server")
  results.signOut = { path: await ctx.pathname(), title: await ctx.app.evaluate("document.title") }
  ctx.check(results.signOut.path === moviesRoute, "the sign-in form appears on the library page")
  await ctx.screenshot("06-signed-out")
  await ctx.signIn({ username: "demo" })
  results.signInAgain = await ctx.pathname()
  ctx.check(results.signInAgain === moviesRoute, "signing in again stays on the library page")
  await ctx.writeEvidence("library-filters.json", results)
}
