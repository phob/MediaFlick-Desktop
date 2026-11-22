function countMatches(text, pattern) {
  return [...text.matchAll(pattern)].length
}

function triggerRendererCrash(ctx) {
  ctx.step("triggering renderer crash via CDP Page.crash")
  void ctx.app.send("Page.crash").catch(() => {})
}

export default async function rendererCrash(ctx) {
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the initial sign-in screen", 30000)
  await ctx.find({ role: "textbox", name: "Server" })
  ctx.check(await ctx.windowShown(), "the main window is shown before crashing")
  ctx.check(countMatches(ctx.appLog(), /renderer process terminated/g) === 0, "app.log starts with zero renderer terminations")

  triggerRendererCrash(ctx)
  await ctx.until(
    () => countMatches(ctx.appLog(), /action=ReloadApp/g) >= 1,
    "renderer crash auto-reload (action=ReloadApp) in app.log",
    15000,
  )
  await ctx.reconnect((target) => target.url.startsWith("mediaflick-desktop://app"), { timeout: 20000 })
  await ctx.until("document.title === 'Sign in — MediaFlick'", "sign-in screen after first auto-reload", 20000)
  await ctx.find({ role: "textbox", name: "Server" })
  const statusAfterFirstReload = await ctx.api("/api/status")
  ctx.check(statusAfterFirstReload.status === 200, `API /api/status answers 200 after first auto-reload (got ${statusAfterFirstReload.status})`)
  const windowShownAfterFirstReload = await ctx.windowShown()
  ctx.check(windowShownAfterFirstReload, "the main window stays shown after first auto-reload")
  await ctx.snapshot("01-after-auto-reload")
  await ctx.screenshot("01-after-auto-reload")

  triggerRendererCrash(ctx)
  await ctx.until(
    () => countMatches(ctx.appLog(), /action=ReloadApp/g) >= 2,
    "second renderer crash auto-reload (action=ReloadApp) in app.log",
    15000,
  )
  await ctx.reconnect((target) => target.url.startsWith("mediaflick-desktop://app"), { timeout: 20000 })
  await ctx.until("document.title === 'Sign in — MediaFlick'", "sign-in screen after second auto-reload", 20000)
  await ctx.find({ role: "textbox", name: "Server" })

  triggerRendererCrash(ctx)
  await ctx.until(
    () => countMatches(ctx.appLog(), /action=ShowErrorPage/g) >= 1,
    "renderer crash fallback to error page (action=ShowErrorPage) in app.log",
    15000,
  )
  await ctx.reconnect((target) => target.url.startsWith("data:text/html"), { timeout: 20000 })
  await ctx.find({ role: "heading", name: "MediaFlick couldn’t load this page." })
  await ctx.find({ role: "link", name: "Try again" })
  await ctx.find({ role: "link", name: "Open Settings" })
  const windowShownOnErrorPage = await ctx.windowShown()
  ctx.check(windowShownOnErrorPage, "the main window is shown on the native error page")
  await ctx.snapshot("02-error-page")
  await ctx.screenshot("02-error-page")

  await ctx.press({ role: "link", name: "Try again" })
  await ctx.reconnect((target) => target.url.startsWith("mediaflick-desktop://app"), { timeout: 20000 })
  await ctx.until("document.title === 'Sign in — MediaFlick'", "sign-in screen after clicking Try again", 20000)
  await ctx.find({ role: "textbox", name: "Server" })
  const statusAfterRetry = await ctx.api("/api/status")
  ctx.check(statusAfterRetry.status === 200, `API /api/status answers 200 after clicking Try again (got ${statusAfterRetry.status})`)
  const windowShownAfterRetry = await ctx.windowShown()
  ctx.check(windowShownAfterRetry, "the main window stays shown after clicking Try again")
  await ctx.snapshot("03-recovered-after-retry")
  await ctx.screenshot("03-recovered-after-retry")

  const log = ctx.appLog()
  const reloadLogCount = countMatches(log, /action=ReloadApp/g)
  const errorPageLogCount = countMatches(log, /action=ShowErrorPage/g)
  ctx.check(reloadLogCount === 2, `app.log recorded 2 ReloadApp actions (got ${reloadLogCount})`)
  ctx.check(errorPageLogCount === 1, `app.log recorded 1 ShowErrorPage action (got ${errorPageLogCount})`)

  ctx.writeEvidence("renderer-crash.json", {
    reloadLogCount,
    errorPageLogCount,
    statusAfterFirstReload: statusAfterFirstReload.status,
    statusAfterRetry: statusAfterRetry.status,
    windowShownAfterFirstReload,
    windowShownOnErrorPage,
    windowShownAfterRetry,
  })
}
