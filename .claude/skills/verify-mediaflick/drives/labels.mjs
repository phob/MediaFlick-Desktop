// The UI's shared labels against Jellyfin's public demo server, read-only:
// the sign-in screen's settings link and the way back, the settings save bar
// and its disabled Save, Favorites, the watched and favorites toggles on a
// movie and on episode cards, and the Releases startup destination (opened,
// never chosen). Never presses Play, a favorites toggle or a watched toggle.
//   just verify labels
const DEMO = "https://demo.jellyfin.org/stable"
const MOVIE = "Night of the Living Dead"
const SERIES = "Pioneer One"
const WATCHED_ACTIONS = new Set(["Mark watched", "Mark unwatched"])

async function open(ctx, term, title) {
  await ctx.park()
  if ((await ctx.pathname()) !== "/") await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.fill({ role: "textbox", name: "Search the library" }, term)
  await ctx.until(async () => (await ctx.pathname()) === `/library?search=${encodeURIComponent(term)}`, "the search route")
  await ctx.press({ role: "link", name: `Open details for ${title}` }, { timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/item/"), `the ${title} page`)
}

const saveLook = (ctx) => ctx.app.evaluate(`(() => {
  const save = [...document.querySelectorAll(".settings-save-bar button")].find((b) => b.textContent.trim() === "Save")
  const probe = document.createElement("div")
  probe.className = "bg-muted text-muted-foreground"
  document.body.append(probe)
  const muted = getComputedStyle(probe)
  const style = getComputedStyle(save)
  const result = {
    disabled: save.disabled,
    background: style.backgroundColor,
    color: style.color,
    opacity: style.opacity,
    mutedBackground: muted.backgroundColor,
    mutedForeground: muted.color,
  }
  probe.remove()
  return result
})()`)

export default async function labels(ctx) {
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  await ctx.find({ role: "textbox", name: "Server" })
  await ctx.find({ role: "link", name: "Player and app settings" })
  await ctx.screenshot("01-sign-in")

  await ctx.press({ role: "link", name: "Player and app settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "Player settings while signed out")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  await ctx.find({ role: "button", name: "Restore defaults" })
  ctx.check(!(await ctx.exists({ role: "button", name: "Reset" })), "no button is named Reset")
  ctx.check((await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save is disabled with no changes")
  const look = await saveLook(ctx)
  ctx.writeEvidence("save-button-look.json", JSON.stringify(look, null, 2))
  ctx.check(look.background === look.mutedBackground && look.color === look.mutedForeground && look.opacity === "1", `disabled Save uses the muted tokens at full opacity (${look.background}, ${look.color})`)
  await ctx.snapshot("02-player-signed-out")
  await ctx.screenshot("02-player-signed-out")

  await ctx.press({ role: "link", name: "Back to sign in" })
  await ctx.find({ role: "textbox", name: "Server" }, { timeout: 30000 })
  await ctx.screenshot("03-back-to-sign-in")

  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  ctx.step(`shelves: ${JSON.stringify(await ctx.app.evaluate(`[...document.querySelectorAll("section[aria-labelledby] h2")].map((h) => h.textContent.trim())`))}`)
  ctx.check(!(await ctx.app.evaluate(`document.body.innerText.includes("My List")`)), "Home never says My List")
  await ctx.press({ role: "link", name: "Favorites" })
  await ctx.until(async () => (await ctx.pathname()) === "/library?favorite=true", "the Favorites route")
  await ctx.find({ role: "heading", name: "Favorites" })
  await ctx.find({ role: "button", name: "Remove Favorites filter" })
  await ctx.park()
  await ctx.screenshot("04-favorites")

  await open(ctx, "night of the living", MOVIE)
  await ctx.find({ role: "heading", name: MOVIE })
  const watched = (await ctx.exists({ role: "button", name: "Mark watched" })) ? "Mark watched" : "Mark unwatched"
  const watchedState = await ctx.state({ role: "button", name: watched })
  ctx.check(watchedState.pressed === (watched === "Mark unwatched"), `the movie offers "${watched}" with aria-pressed matching its state`)
  ctx.check((await ctx.exists({ role: "button", name: "Add to favorites" })) || (await ctx.exists({ role: "button", name: "Remove from favorites" })), "the movie's favorites toggle")
  await ctx.snapshot("05-movie")
  await ctx.screenshot("05-movie")

  await open(ctx, SERIES, SERIES)
  await ctx.find({ role: "heading", name: SERIES })
  const series = await ctx.until(async () => {
    const tree = await ctx.snapshot("06-series")
    return /link '\d+\./.test(tree) ? tree : null
  }, "the episode list", 30000)
  const watchButtons = [...series.matchAll(/button '([^']*watch[^']*)'/gi)].map((match) => match[1])
  ctx.writeEvidence("episode-watch-buttons.json", JSON.stringify(watchButtons, null, 2))
  ctx.check(watchButtons.length > 0, `the series page has watched toggles (${watchButtons.length})`)
  ctx.check(watchButtons.every((name) => WATCHED_ACTIONS.has(name)), `every watched toggle is Mark watched or Mark unwatched: ${JSON.stringify([...new Set(watchButtons)])}`)
  // An episode title link's name gains " (Next up)" whenever that query lands,
  // so target the first title link by its number prefix instead of its name.
  await ctx.hover({ css: 'a[href^="/item/"]:has(> span.tabular-nums)' }, { nth: 0 })
  await ctx.screenshot("06-series-episode-actions")
  await ctx.park()

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Viewing" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/viewing", "Viewing settings")
  await ctx.find({ role: "combobox", name: "Startup destination" }, { timeout: 30000 })
  const nextEpisode = (await ctx.state({ role: "combobox", name: "Next episode" })).value
  const countdown = await ctx.state({ role: "spinbutton", name: "Countdown seconds" })
  ctx.check(countdown.disabled === (nextEpisode !== "Ask with countdown"), `Countdown seconds is ${countdown.disabled ? "disabled" : "enabled"} with Next episode "${nextEpisode}"`)
  await ctx.press({ role: "combobox", name: "Startup destination" })
  await ctx.find({ role: "option", name: "Releases" })
  ctx.check(!(await ctx.exists({ role: "option", name: "Calendar" })), "no Calendar option")
  await ctx.snapshot("07-startup-destination")
  await ctx.screenshot("07-startup-destination")
  await ctx.key("Escape", { code: "Escape", keyCode: 27 })
  await ctx.until(async () => !(await ctx.exists({ role: "listbox" })), "the select to close")
  await ctx.find({ role: "button", name: "Restore defaults" })
  ctx.check((await ctx.state({ role: "button", name: "Save" })).disabled === true, "Viewing has no unsaved draft")
}
