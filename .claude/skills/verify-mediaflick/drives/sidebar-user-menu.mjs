// The sidebar's user menu off Home, where the sidebar expands under the
// pointer and collapses when it leaves. The open menu must hold the sidebar
// expanded, also while the pointer rests on the menu, and the sidebar must
// collapse again once the menu closes and the pointer is away.
//   just verify sidebar-user-menu
const DEMO = "https://demo.jellyfin.org/stable"
const USER_MENU = { role: "button", name: "D demo demo.jellyfin.org" }
// The menu's marker. Not `Sync library`: that item reads `Syncing…` while a
// sync runs, which sign-in starts.
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
  await ctx.until(async () => (await ctx.pathname()) === "/library?kind=Movie", "the Movies library")
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
}
