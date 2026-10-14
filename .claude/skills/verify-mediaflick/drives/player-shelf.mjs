// The Player shelf's own save path: the shortcut conflict alert, the Player
// toast, and Restore defaults + Save, which keeps player_backend, comfort and
// mark_watched_next in settings.json. Device-local; the demo server only
// provides the sign-in.
//   just verify player-shelf
const DEMO = "https://demo.jellyfin.org/stable"
const alerts = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll("[role=alert]")].map((e) => e.textContent.trim()).filter(Boolean).join(" | ") || null`)
const toasts = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll("[data-sonner-toast]")].map((e) => e.textContent.trim()).join(" | ") || null`)

export default async function playerShelf(ctx) {
  const results = {}
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const { capabilities } = (await ctx.api("/api/settings")).body
  ctx.step(`capabilities ${JSON.stringify(capabilities)}`)
  results.settingsFresh = ctx.readConfig("settings.json")

  if (capabilities.libmpv) {
    await ctx.press({ role: "button", name: "Pause key" })
    ctx.check((await ctx.state({ role: "button", name: "Pause key" })).pressed === true, "Pause key is recording")
    await ctx.key("m", { code: "KeyM", keyCode: 77 })
    results.conflictAlert = await ctx.until(() => alerts(ctx), "a conflict alert")
    ctx.check(results.conflictAlert === "Assign each key combination to only one action.", "the conflict alert")
    ctx.check((await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save is disabled on a conflict")
    await ctx.screenshot("01-shortcut-conflict")
    await ctx.press({ role: "button", name: "Discard" })
  }

  await ctx.choose({ name: "Default fullscreen" }, "Windowed")
  await ctx.press({ role: "button", name: "Save" })
  results.playerToast = await ctx.until(() => toasts(ctx), "the save toast")
  ctx.check(results.playerToast.includes("Player settings saved"), "the Player shelf's own toast")
  await ctx.until(() => ctx.readConfig("settings.json")?.default_fullscreen === "windowed", "settings.json to record default_fullscreen")
  results.settingsSaved = ctx.readConfig("settings.json")
  await ctx.screenshot("02-player-saved")

  await ctx.press({ role: "button", name: "Restore defaults" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => !ctx.readConfig("settings.json")?.default_fullscreen, "default_fullscreen to leave settings.json")
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save to disable once clean")
  results.settingsRestored = ctx.readConfig("settings.json")
  for (const key of ["player_backend", "comfort", "mark_watched_next"]) {
    ctx.check(key in results.settingsRestored, `Restore defaults + Save keeps ${key}`)
  }
  await ctx.writeEvidence("player-shelf.json", results)
}
