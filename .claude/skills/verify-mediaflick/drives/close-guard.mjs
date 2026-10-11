// The unsaved-settings guard on a window-manager close (the title bar's close
// button): an unsaved draft asks "Leave without saving?" first. Keep editing
// keeps the app and the draft; Discard and leave exits without writing the
// draft. Device-local; the demo server only provides the sign-in. Linux only
// (wmClose).
//   just verify close-guard
const DEMO = "https://demo.jellyfin.org/stable"
const SWITCH = { role: "switch", name: "Show scrollbars" }
const GUARD = { role: "dialog", name: "Leave without saving?" }
// role=status takes no accessible name from its content, so read its text.
const saveBarStatus = (ctx) => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)

export default async function closeGuard(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "the Application shelf")
  const { client } = (await ctx.api("/api/settings")).body
  ctx.step(`backend ${client.player.playerBackend}`)
  ctx.check(ctx.readConfig("settings.json")?.show_scrollbars !== true, "settings.json starts without show_scrollbars")

  await ctx.press(SWITCH)
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the unsaved-changes notice")
  await ctx.screenshot("01-draft")

  ctx.wmClose()
  await ctx.find(GUARD)
  await ctx.snapshot("02-close-guard")
  await ctx.screenshot("02-close-guard")
  await ctx.press({ role: "button", name: "Keep editing" })
  ctx.check((await ctx.appExit(3000)) === null, "Keep editing keeps the app running")
  ctx.check((await ctx.state(SWITCH)).checked === true, "the draft keeps Show scrollbars on")
  ctx.check((await saveBarStatus(ctx)) === "You have unsaved changes.", "the draft is still unsaved")
  await ctx.snapshot("03-kept-editing")

  ctx.wmClose()
  await ctx.find(GUARD)
  await ctx.press({ role: "button", name: "Discard and leave" })
  ctx.check((await ctx.appExit(20000)) !== null, "Discard and leave exits the app")
  const after = ctx.readConfig("settings.json")
  ctx.writeEvidence("settings-after.json", after ?? {})
  ctx.check(after?.show_scrollbars !== true, "settings.json did not record the discarded draft")
}
