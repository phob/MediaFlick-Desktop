const DEMO = "https://demo.jellyfin.org/stable"
const SWITCH = { role: "switch", name: "Show scrollbars" }
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

  await ctx.press(SWITCH)
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "the switch to turn on")
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the save bar to report unsaved changes")
  ctx.check(!(await ctx.state({ role: "button", name: "Save" })).disabled, "Save is enabled for the draft")
  ctx.check(ctx.configStat("settings.json")?.mtimeMs === beforeStat?.mtimeMs, "settings.json is untouched while the change is a draft")
  await ctx.snapshot("02-draft")
  await ctx.screenshot("02-draft")

  await ctx.press({ role: "button", name: "Discard" })
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === false, "Discard to restore the saved value")
  ctx.check((await saveBarStatus(ctx)) === "", "the unsaved-changes notice is gone after Discard")
  ctx.check(ctx.configStat("settings.json")?.mtimeMs === beforeStat?.mtimeMs, "Discard did not write settings.json")

  await ctx.press(SWITCH)
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "the switch to turn on again")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.show_scrollbars === true, "settings.json to record show_scrollbars: true")
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "Save to disable once clean")
  const saved = await ctx.api("/api/settings")
  ctx.check(saved.body?.client?.application?.showScrollbars === true, "/api/settings reports showScrollbars: true")
  await ctx.screenshot("03-saved")

  await ctx.press({ role: "link", name: "Player" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "the Player shelf")
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "Show scrollbars to read back as on")
  await ctx.snapshot("04-reopened")

  const DELETE_INPUT = { role: "textbox", name: "Type DELETE to confirm local account deletion" }
  const DELETE_TRIGGER = { role: "button", name: "Delete local account data" }
  const DELETE_DIALOG = { role: "alertdialog", name: "Delete local account data?" }

  await ctx.press(SWITCH)
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === false, "Show scrollbars toggled off as an unsaved draft")
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "unsaved-changes notice before local account deletion")
  await ctx.fill(DELETE_INPUT, "DELETE")
  await ctx.press(DELETE_TRIGGER)
  await ctx.find(DELETE_DIALOG)
  const hasKeepEditing = await ctx.exists({ role: "button", name: "Keep editing", within: DELETE_DIALOG })
  const hasDiscardAndDelete = await ctx.exists({ role: "button", name: "Discard and delete", within: DELETE_DIALOG })
  if (!hasKeepEditing || !hasDiscardAndDelete) {
    if (await ctx.exists({ role: "button", name: "Cancel", within: DELETE_DIALOG })) {
      await ctx.press({ role: "button", name: "Cancel", within: DELETE_DIALOG })
    }
    if (await ctx.exists({ role: "button", name: "Discard" })) {
      await ctx.press({ role: "button", name: "Discard" })
    }
    ctx.check(false, "Delete local account data dialog warns about unsaved Application settings with Keep editing and Discard and delete")
  }
  await ctx.snapshot("05-delete-dirty-guard")
  await ctx.screenshot("05-delete-dirty-guard")
  await ctx.press({ role: "button", name: "Keep editing", within: DELETE_DIALOG })
  await ctx.until(async () => !(await ctx.exists(DELETE_DIALOG)), "delete confirmation dialog to close on Keep editing")
  ctx.check((await ctx.api("/api/status")).body?.authenticated === true, "Keep editing on dirty delete keeps the account signed in")
  ctx.check((await ctx.state(SWITCH)).checked === false, "Keep editing preserves the dirty Show scrollbars draft")
  ctx.check((await saveBarStatus(ctx)) === "You have unsaved changes.", "Keep editing keeps the unsaved-changes notice")

  await ctx.press(DELETE_TRIGGER)
  await ctx.find(DELETE_DIALOG)
  await ctx.press({ role: "button", name: "Discard and delete", within: DELETE_DIALOG })
  await ctx.until(async () => (await ctx.api("/api/status")).body?.authenticated === false, "Discard and delete to sign the device out")
  await ctx.find({ role: "link", name: "Back to sign in" })
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "Discard and delete restores the saved Show scrollbars value")
  ctx.check((await saveBarStatus(ctx)) === "", "the unsaved-changes notice is cleared after Discard and delete")
  ctx.check(ctx.readConfig("settings.json")?.show_scrollbars === true, "settings.json kept the saved show_scrollbars value and did not write the discarded draft")
  await ctx.screenshot("06-after-delete-discard")
  ctx.writeEvidence("settings-before.json", before ?? {})
  ctx.writeEvidence("settings-after.json", ctx.readConfig("settings.json"))
}
