// The Client settings shelves beyond Show scrollbars: the Player shelf for
// each backend, a range alert, the leave-without-saving guard, a Playback
// select, restart notices, the live scrollbar style, Reset + Save, and the
// Delete local account data dialog (cancelled). Device-local; the demo server
// only provides the sign-in. Never presses Installation help or Choose mpv
// executable.
//   just verify client-settings
const DEMO = "https://demo.jellyfin.org/stable"
// role=status takes no accessible name from its content, so read its text.
const saveBarStatus = (ctx) => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)
// Present while scrollbars are hidden; saving show_scrollbars removes it live.
const scrollbarStyle = (ctx) => ctx.app.evaluate(`!!document.getElementById("__mediaFlickDesktopScrollbarStyle")`)

export default async function clientSettings(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const { capabilities, client } = (await ctx.api("/api/settings")).body
  ctx.step(`capabilities ${JSON.stringify(capabilities)}, backend ${client.player.playerBackend}`)
  await ctx.snapshot("01-player")
  await ctx.screenshot("01-player")

  // Built-in player: comfort values and shortcut recorders.
  if (capabilities.libmpv) {
    ctx.check((await ctx.state({ role: "combobox", name: "Player backend" })).value === "Built-in player", "Built-in player is selected")
    await ctx.find({ role: "button", name: "Pause key" })
    await ctx.fill({ role: "spinbutton", name: "Subtitle size (%)" }, "300")
    const alert = await ctx.until(() => ctx.app.evaluate(`[...document.querySelectorAll("[role=alert]")].map((e) => e.textContent.trim()).join(" | ") || null`), "a range alert")
    ctx.step(`alert: ${alert}`)
    ctx.check((await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save is disabled while a value is out of range")
    await ctx.press({ role: "button", name: "Discard" })
  }

  // External mpv: path controls; the installer button exists only on Windows.
  const saved = ctx.configStat("settings.json")?.mtimeMs
  await ctx.choose({ name: "Player backend" }, "External mpv")
  await ctx.find({ role: "textbox", name: "mpv executable" })
  await ctx.find({ role: "button", name: "Choose mpv executable" })
  await ctx.find({ role: "button", name: "Mark watched key" })
  ctx.check(!(await ctx.exists({ role: "button", name: "Pause key" })), "player shortcut recorders are built-in only")
  ctx.check((await ctx.exists({ role: "button", name: "Install mpv" })) === capabilities.mpvInstaller, "Install mpv follows capabilities.mpvInstaller")
  ctx.step(`status: ${await saveBarStatus(ctx)}`)
  await ctx.snapshot("02-external-mpv")
  await ctx.screenshot("02-external-mpv")
  await ctx.press({ role: "button", name: "Discard" })
  ctx.check(ctx.configStat("settings.json")?.mtimeMs === saved, "Discard leaves settings.json alone")

  // Leaving a dirty shelf asks first.
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "the Application shelf")
  await ctx.press({ role: "switch", name: "Show scrollbars" })
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the unsaved-changes notice")
  await ctx.press({ role: "link", name: "Playback" })
  await ctx.find({ role: "dialog", name: "Leave without saving?" })
  await ctx.screenshot("03-leave-guard")
  await ctx.press({ role: "button", name: "Keep editing" })
  ctx.check((await ctx.pathname()) === "/settings/client/application", "Keep editing stays on the shelf")
  await ctx.press({ role: "link", name: "Playback" })
  await ctx.press({ role: "button", name: "Discard and leave" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/playback", "Discard and leave opens Playback")

  await ctx.choose({ name: "Intro skipping" }, "Always skip")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.skip_intro === "always", "settings.json to record skip_intro: always")

  // A restart notice replaces the unsaved-changes text; scrollbars apply live.
  await ctx.press({ role: "link", name: "Application" })
  ctx.check((await ctx.state({ role: "switch", name: "Show scrollbars" })).checked === false, "Discard and leave dropped the draft")
  ctx.check(await scrollbarStyle(ctx), "the scrollbar-hiding style is present")
  await ctx.press({ role: "switch", name: "Show scrollbars" })
  await ctx.choose({ name: "Log level" }, "Info")
  await ctx.until(async () => (await saveBarStatus(ctx)) === "Log-level changes apply after restarting MediaFlick.", "the log-level restart notice")
  // The "Settings saved" toast then covers Save for a few seconds; press waits.
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.show_scrollbars === true && ctx.readConfig("settings.json")?.log_level === "info", "settings.json to record both values")
  await ctx.until(async () => !(await scrollbarStyle(ctx)), "the scrollbar style to go without a restart")
  await ctx.screenshot("04-saved")

  // Reset loads defaults into the draft; Save then drops the keys.
  await ctx.press({ role: "button", name: "Reset" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => !("show_scrollbars" in (ctx.readConfig("settings.json") ?? {})), "Reset + Save to drop show_scrollbars")
  await ctx.until(() => scrollbarStyle(ctx), "the scrollbar style to return")

  // Typing DELETE only enables the button; the wipe needs the dialog's action.
  await ctx.fill({ role: "textbox", name: "Type DELETE to confirm local account deletion" }, "DELETE")
  await ctx.press({ role: "button", name: "Delete local account data" })
  await ctx.find({ role: "alertdialog", name: "Delete local account data?" })
  await ctx.screenshot("05-delete-dialog")
  await ctx.press({ role: "button", name: "Cancel" })
  await ctx.until(async () => !(await ctx.exists({ role: "alertdialog" })), "the dialog to close")
  ctx.check((await ctx.api("/api/status")).body?.authenticated === true, "still signed in after Cancel")
  ctx.writeEvidence("settings-final.json", ctx.readConfig("settings.json"))
}
