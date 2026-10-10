// Settings → Application through the real path: sign in (Jellyfin's public
// demo server, inside the disposable profile), open Settings from the
// sidebar, change "Show scrollbars", prove Discard leaves settings.json alone
// and Save writes it, and that the saved value survives leaving the page.
//   just verify application-settings
const DEMO = "https://demo.jellyfin.org/stable"
const SWITCH = { role: "switch", name: "Show scrollbars" }
// role=status takes no accessible name from its content, so read its text.
const saveBarStatus = (ctx) => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)

export default async function applicationSettings(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf (settings index)")
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "the Application shelf")
  await ctx.find(SWITCH)

  const before = ctx.readConfig("settings.json")
  const beforeStat = ctx.configStat("settings.json")
  ctx.check(before?.show_scrollbars !== true, "settings.json starts without show_scrollbars")
  ctx.check((await ctx.state(SWITCH)).checked === false, "Show scrollbars starts off")
  await ctx.screenshot("01-application-clean")

  // A change only becomes a draft.
  await ctx.press(SWITCH)
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "the switch to turn on")
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the save bar to report unsaved changes")
  ctx.check(!(await ctx.state({ role: "button", name: "Save" })).disabled, "Save is enabled for the draft")
  ctx.check(ctx.configStat("settings.json")?.mtimeMs === beforeStat?.mtimeMs, "settings.json is untouched while the change is a draft")
  await ctx.snapshot("02-draft")
  await ctx.screenshot("02-draft")

  // Discard returns to the saved value without writing.
  await ctx.press({ role: "button", name: "Discard" })
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === false, "Discard to restore the saved value")
  ctx.check((await saveBarStatus(ctx)) === "", "the unsaved-changes notice is gone after Discard")
  ctx.check(ctx.configStat("settings.json")?.mtimeMs === beforeStat?.mtimeMs, "Discard did not write settings.json")

  // Save writes the file.
  await ctx.press(SWITCH)
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "the switch to turn on again")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.show_scrollbars === true, "settings.json to record show_scrollbars: true")
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save to disable once clean")
  const saved = await ctx.api("/api/settings")
  ctx.check(saved.body?.client?.application?.showScrollbars === true, "/api/settings reports showScrollbars: true")
  await ctx.screenshot("03-saved")

  // Leaving and returning shows the stored value, not a stale draft.
  await ctx.press({ role: "link", name: "Player" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf")
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "Show scrollbars to read back as on")
  await ctx.snapshot("04-reopened")
  ctx.writeEvidence("settings-before.json", before ?? {})
  ctx.writeEvidence("settings-after.json", ctx.readConfig("settings.json"))
}
