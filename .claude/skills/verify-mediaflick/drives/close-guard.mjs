import { execFileSync } from "node:child_process"

const DEMO = "https://demo.jellyfin.org/stable"
const SWITCH = { role: "switch", name: "Show scrollbars" }
const GUARD = { role: "dialog", name: "Leave without saving?" }
const SAVE = { role: "button", name: "Save" }
const saveBarStatus = (ctx) => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)
const headingPoint = `(() => { const r = document.querySelector(".settings-main h1")?.getBoundingClientRect(); return r ? { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) } : null })()`
const xdotool = (...args) => execFileSync("xdotool", args.map(String), { encoding: "utf8" })

function mainWindowId() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) .*?(\d+)x(\d+)\+/gm)]
    .map(([, id, width, height]) => ({ id, area: Number(width) * Number(height) }))
    .sort((a, b) => b.area - a.area)[0].id
}

function visibleWindows() {
  const tree = execFileSync("xwininfo", ["-root", "-children"], { encoding: "utf8" })
  return [...tree.matchAll(/^\s+(0x[0-9a-f]+) /gm)].flatMap(([, id]) => {
    const info = execFileSync("xwininfo", ["-id", id], { encoding: "utf8" })
    if (!/Map State: IsViewable/.test(info)) return []
    const size = /Width: (\d+)[\s\S]*Height: (\d+)/.exec(info)
    return [{ id, width: Number(size[1]), height: Number(size[2]) }]
  })
}

async function chooseContextMenuSettings(ctx, windowId, captureName) {
  const point = await ctx.until(headingPoint, "the settings heading")
  const before = visibleWindows()
  xdotool("mousemove", "--window", windowId, point.x, point.y, "click", 3)
  const menu = await ctx.until(() => {
    const added = visibleWindows().filter((w) => !before.some((b) => b.id === w.id))
    return added.length > 0 ? added[0] : null
  }, "the native context menu to open", 10000)
  const clickX = Math.round(menu.width / 2)
  const clickY = Math.round(menu.height * 0.61)
  xdotool("mousemove", "--window", menu.id, clickX, clickY)
  if (captureName) ctx.displayCapture(captureName)
  xdotool("click", 1)
  await ctx.until(() => visibleWindows().length === before.length, "the native context menu to close", 10000)
}

export default async function closeGuard(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "the Application shelf")
  const { client } = (await ctx.api("/api/settings")).body
  ctx.step(`backend ${client.player.playerBackend}`)
  ctx.check(ctx.readConfig("settings.json")?.show_scrollbars !== true, "settings.json starts without show_scrollbars")

  const windowId = mainWindowId()

  await ctx.press(SWITCH)
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the unsaved-changes notice")
  await ctx.screenshot("01-draft")

  await chooseContextMenuSettings(ctx, windowId, "01b-context-menu")
  await ctx.find(GUARD)
  await ctx.snapshot("01c-context-menu-guard")
  await ctx.screenshot("01c-context-menu-guard")
  await ctx.press({ role: "button", name: "Keep editing" })
  await ctx.until(async () => !(await ctx.exists(GUARD)), "the leave dialog to close")
  ctx.check((await ctx.appExit(2000)) === null, "Keep editing after context-menu Settings keeps the app running")
  ctx.check((await ctx.pathname()) === "/settings/client/application", "Keep editing stays on the Application shelf")
  ctx.check((await ctx.state(SWITCH)).checked === true, "Keep editing keeps Show scrollbars on")

  await chooseContextMenuSettings(ctx, windowId, null)
  await ctx.find(GUARD)
  await ctx.press({ role: "button", name: "Discard and leave" })
  ctx.check((await ctx.appExit(3000)) === null, "Discard and leave from context-menu Settings keeps the app running")
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "navigation to the Player shelf")
  ctx.check(ctx.readConfig("settings.json")?.show_scrollbars !== true, "settings.json did not record the discarded Application draft")
  await ctx.screenshot("01d-after-context-menu-discard")

  await ctx.find({ role: "combobox", name: "Default fullscreen" })
  const nextFullscreen = client.player.defaultFullscreen === "windowed" ? "Fullscreen" : "Windowed"
  await ctx.choose({ name: "Default fullscreen" }, nextFullscreen)
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the Player unsaved-changes notice")

  await chooseContextMenuSettings(ctx, windowId, null)
  ctx.check((await ctx.appExit(2000)) === null, "context-menu Settings on dirty Player shelf keeps the app running")
  ctx.check(!(await ctx.exists(GUARD)), "context-menu Settings on dirty Player shelf opens no leave dialog")
  ctx.check((await ctx.pathname()) === "/settings/client/player", "context-menu Settings stays on the Player shelf")
  ctx.check(
    !(await ctx.state(SAVE)).disabled
      && (await saveBarStatus(ctx)) === "You have unsaved changes."
      && (await ctx.state({ role: "combobox", name: "Default fullscreen" })).value === nextFullscreen,
    "context-menu Settings keeps the dirty Player draft intact",
  )
  await ctx.press({ role: "button", name: "Discard" })
  await ctx.until(async () => Boolean((await ctx.state(SAVE)).disabled), "the Player draft to discard")

  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "return to the Application shelf")
  await ctx.press(SWITCH)
  await ctx.until(async () => (await saveBarStatus(ctx)) === "You have unsaved changes.", "the unsaved-changes notice before window close")

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
