const DEMO = "https://demo.jellyfin.org/stable"
const USER_MENU = { role: "button", name: "D demo demo.jellyfin.org" }
const SIGN_OUT = { role: "menuitem", name: "Sign out" }
const sidebarState = (ctx) => ctx.app.evaluate(`document.querySelector('[data-slot="sidebar"]')?.dataset.state ?? null`)
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function sample(ctx, durationMs, move) {
  const states = []
  const end = Date.now() + durationMs
  for (let tick = 0; Date.now() < end; tick++) {
    if (move) await move(tick)
    states.push(await sidebarState(ctx))
    await sleep(50)
  }
  return states
}

const collapses = (states) => states.filter((state) => state !== "expanded").length

export default async function sidebarUserMenu(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Movies" })
  await ctx.until(async () => (await ctx.pathname()).startsWith("/library/"), "the Movies library")
  await ctx.park()
  await ctx.until(async () => (await sidebarState(ctx)) === "collapsed", "the sidebar collapsed off Home")

  await ctx.hover(USER_MENU)
  await ctx.until(async () => (await sidebarState(ctx)) === "expanded", "the sidebar expanded under the pointer")
  await ctx.press(USER_MENU)
  await ctx.find(SIGN_OUT)
  const afterOpen = await sample(ctx, 600)
  await ctx.screenshot("01-menu-open")

  const item = await ctx.find(SIGN_OUT)
  ctx.step(`pointer onto ${SIGN_OUT.name} at ${Math.round(item.rect.x)},${Math.round(item.rect.y)}`)
  const onMenu = await sample(ctx, 1500, (tick) => ctx.app.send("Input.dispatchMouseEvent", {
    type: "mouseMoved",
    x: item.rect.x + (tick % 2),
    y: item.rect.y,
    button: "none",
  }))
  await ctx.screenshot("02-pointer-on-menu")
  await ctx.writeEvidence("sidebar-states.json", { afterOpen, onMenu })

  ctx.check(collapses(afterOpen) === 0, `the sidebar stays expanded after the menu opens (${afterOpen.join(",")})`)
  ctx.check(collapses(onMenu) === 0, `the sidebar stays expanded with the pointer on the menu (${onMenu.join(",")})`)
  ctx.check(await ctx.exists(SIGN_OUT), "the menu is still open")

  await ctx.key("Escape")
  await ctx.until(async () => !(await ctx.exists(SIGN_OUT)), "the menu closed")
  await ctx.park()
  await ctx.until(async () => (await sidebarState(ctx)) === "collapsed", "the sidebar collapsed once the menu closed")
  await ctx.screenshot("03-menu-closed")

  const SWITCH = { role: "switch", name: "Show scrollbars" }
  const SIGN_OUT_GUARD = { role: "dialog", name: "Sign out without saving?" }
  const saveBarStatus = () => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)

  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "the Application shelf")
  await ctx.press(SWITCH)
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === true, "Show scrollbars turned on as a draft")
  await ctx.until(async () => (await saveBarStatus()) === "You have unsaved changes.", "unsaved-changes notice before sign-out")

  await ctx.hover(USER_MENU)
  await ctx.press(USER_MENU)
  await ctx.press(SIGN_OUT)
  await ctx.until(
    async () => (await ctx.exists(SIGN_OUT_GUARD)) || (await ctx.api("/api/status")).body?.authenticated === false,
    "either the sign-out draft guard or immediate sign-out",
  )
  ctx.check(
    await ctx.exists(SIGN_OUT_GUARD),
    "Sign out with a dirty Settings shelf opens the Sign out without saving? dialog instead of signing out immediately",
  )
  await ctx.snapshot("04-signout-dirty-guard")
  await ctx.screenshot("04-signout-dirty-guard")
  await ctx.press({ role: "button", name: "Keep editing", within: SIGN_OUT_GUARD })
  await ctx.until(async () => !(await ctx.exists(SIGN_OUT_GUARD)), "sign-out draft guard closed on Keep editing")
  ctx.check((await ctx.api("/api/status")).body?.authenticated === true, "Keep editing on dirty sign-out keeps the session signed in")
  ctx.check((await ctx.state(SWITCH)).checked === true, "Keep editing preserves the dirty Show scrollbars draft")
  ctx.check((await saveBarStatus()) === "You have unsaved changes.", "Keep editing keeps the unsaved-changes notice")

  await ctx.hover(USER_MENU)
  await ctx.press(USER_MENU)
  await ctx.press(SIGN_OUT)
  await ctx.find(SIGN_OUT_GUARD)
  await ctx.press({ role: "button", name: "Discard and sign out", within: SIGN_OUT_GUARD })
  await ctx.until(async () => (await ctx.api("/api/status")).body?.authenticated === false, "Discard and sign out to sign out")
  await ctx.find({ role: "link", name: "Back to sign in" })
  await ctx.until(async () => (await ctx.state(SWITCH)).checked === false, "Discard and sign out resets the discarded Show scrollbars draft")
  ctx.check((await saveBarStatus()) === "", "the unsaved-changes notice is cleared after Discard and sign out")
  ctx.check(ctx.readConfig("settings.json")?.show_scrollbars !== true, "settings.json did not record the discarded draft on sign-out")
  await ctx.screenshot("05-after-signout-discard")
}
