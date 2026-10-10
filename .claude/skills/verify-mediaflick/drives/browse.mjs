// Sign in, browse, sign out against Jellyfin's public demo server, read-only:
// Home shelves, the Movies library, sidebar search, a movie and a series
// detail page, then Sign out from the user menu. The demo account is shared
// by everyone, so this drive never favorites, marks watched or plays.
//   just verify browse
const DEMO = "https://demo.jellyfin.org/stable"
const MOVIE = "Night of the Living Dead"
const SERIES = "Pioneer One"
const CARD = { css: 'a[aria-label^="Open details for"]' }

export default async function browse(ctx) {
  // Sign in: the profile records the account.
  await ctx.signIn({ server: DEMO, username: "demo" })
  const accounts = ctx.readConfig("accounts.json")
  ctx.check(accounts?.accounts?.length === 1, "accounts.json holds one account after sign-in")
  const status = await ctx.api("/api/status")
  ctx.check(status.body?.authenticated === true && status.body?.userName === "demo", "/api/status reports user demo")

  // Home: shelves are regions named by their heading, filled with poster cards.
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.until(async () => (await ctx.count({ role: "region" })) > 0 && (await ctx.count(CARD)) > 0, "Home shelves with poster cards", 60000)
  await ctx.screenshot("01-home")
  await ctx.snapshot("01-home")

  // Movies from the sidebar.
  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?kind=Movie", "the Movies library")
  await ctx.find({ role: "heading", name: "Movies" })
  await ctx.find({ role: "combobox", name: "Sort by" })
  await ctx.until(async () => (await ctx.count(CARD)) > 0, "movie cards in the grid", 30000)
  await ctx.screenshot("02-movies")

  // Sidebar search commits to /library?search= after a short debounce.
  await ctx.fill({ role: "textbox", name: "Search the library" }, "night of the living")
  await ctx.until(async () => (await ctx.pathname()) === "/library?search=night%20of%20the%20living", "the search route")
  await ctx.find({ role: "heading", name: "Results for “night of the living”" })
  await ctx.press({ role: "link", name: `Open details for ${MOVIE}` }, { timeout: 30000 })

  // Movie detail.
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), "a movie detail route")
  await ctx.find({ role: "heading", name: MOVIE })
  // Other demo users leave progress behind, so either label is correct.
  await ctx.until(async () => (await ctx.exists({ role: "button", name: "Play" })) || (await ctx.exists({ role: "button", name: "Resume" })), "the Play or Resume button")
  await ctx.until(async () => (await ctx.exists({ role: "button", name: "Add to favorites" })) || (await ctx.exists({ role: "button", name: "Remove from favorites" })), "the favorite toggle")
  await ctx.find({ role: "combobox", name: "Streaming quality" })
  await ctx.screenshot("03-movie-detail")
  await ctx.snapshot("03-movie-detail")

  // Series detail: seasons rail and episodes. Off Home the sidebar collapses
  // to icons and has no search field until the pointer expands it.
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, SERIES)
  await ctx.press({ role: "link", name: `Open details for ${SERIES}` }, { timeout: 30000 })
  await ctx.find({ role: "heading", name: SERIES })
  await ctx.find({ role: "list", name: "Seasons" }, { timeout: 30000 })
  await ctx.find({ role: "heading", name: "Episodes" })
  await ctx.until(async () => (await ctx.snapshot("04-series-detail")).match(/button '(Play|Resume) S\d+E\d+'/), "the series Play/Resume SxEy button")
  await ctx.screenshot("04-series-detail")

  // Sign out from the user menu (no confirmation dialog). The trigger is named
  // by its avatar initial, user name and server host.
  await ctx.press({ role: "button", name: "D demo demo.jellyfin.org" })
  await ctx.press({ role: "menuitem", name: "Sign out" })
  // Not document.title: it keeps the last page's title after sign-out.
  await ctx.find({ role: "textbox", name: "Server" }, { timeout: 30000 })
  await ctx.find({ role: "button", name: "Sign in" })
  const after = await ctx.api("/api/status")
  ctx.check(after.body?.authenticated === false, "/api/status reports signed out")
  await ctx.screenshot("05-signed-out")
}
