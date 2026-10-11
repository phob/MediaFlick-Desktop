// "When the window closes: Minimize window" through a window-manager close
// (the title bar's close button): the window asks to be minimized and the app
// keeps running. Switched back to "Exit MediaFlick", the next close exits.
// Device-local; the demo server only provides the sign-in. Linux only (wmClose).
//   just verify close-minimize
const DEMO = "https://demo.jellyfin.org/stable"
const CLOSE_BEHAVIOR = { name: "Close behavior" }
const MINIMIZED_LOG = "close request minimized the window"

async function saveCloseBehavior(ctx, option, stored) {
  await ctx.choose(CLOSE_BEHAVIOR, option)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.readConfig("settings.json")?.close_behavior === stored, `settings.json to record close_behavior ${stored ?? "(default)"}`)
}

export default async function closeMinimize(ctx) {
  await ctx.signIn({ server: DEMO, username: "demo" })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Application" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/application", "the Application shelf")
  const { client } = (await ctx.api("/api/settings")).body
  ctx.step(`backend ${client.player.playerBackend}`)

  await saveCloseBehavior(ctx, "Minimize window", "minimize_window")
  await ctx.screenshot("01-minimize-saved")
  const minimized = ctx.wmClose()
  ctx.check(minimized.iconifyRequested, "the window asked the window manager to minimize it")
  ctx.check((await ctx.appExit(5000)) === null, "the app is still running 5 s after the close request")
  ctx.check((await ctx.pathname()) === "/settings/client/application", "the page is still live on the Application shelf")
  await ctx.until(() => ctx.appLog().includes(MINIMIZED_LOG), `app.log to record "${MINIMIZED_LOG}"`)
  await ctx.snapshot("02-after-minimize")

  await saveCloseBehavior(ctx, "Exit MediaFlick", undefined)
  await ctx.screenshot("03-exit-saved")
  const exited = ctx.wmClose()
  ctx.check(!exited.iconifyRequested, "the window did not ask to be minimized")
  ctx.check((await ctx.appExit(20000)) !== null, "the app exits on the close request")
}
