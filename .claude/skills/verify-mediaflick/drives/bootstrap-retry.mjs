// A server that accepts sign-in but cannot deliver the first catalog page.
// The user gets a retry screen with a plain reason, the next attempt, Retry
// now and Sign out, also after a relaunch on the same profile; Retry now loads
// Home once the server recovers. A loopback proxy in front of the demo server
// drops only the bootstrap's catalog pages. Read-only against the demo server.
//   just verify bootstrap-retry
const DEMO = "https://demo.jellyfin.org/stable"
const SERVER = { role: "textbox", name: "Server" }
const COVER = { role: "progressbar", name: "Loading MediaFlick" }
const REASON = { role: "heading", name: "Can't reach your Jellyfin server" }
const RETRY = { role: "button", name: "Retry now" }
const SIGN_OUT = { role: "button", name: "Sign out" }

// The bootstrap pages the catalog in ascending DateCreated order
// (`fetch_items_page` in src/jellyfin/api/items.rs); the incremental sweep
// pages descending, and sign-in and user lookups use other paths.
const bootstrapPage = (url) =>
  url.pathname === "/Items" && url.searchParams.get("SortBy") === "DateCreated" && url.searchParams.get("SortOrder") === "Ascending"

const status = async (ctx) => (await ctx.api("/api/status")).body
const pageText = (ctx) => ctx.app.evaluate("document.body.innerText")

async function signInThrough(ctx, proxy) {
  await ctx.find({ role: "button", name: "Sign in" }, { timeout: 30000 })
  await ctx.fill(SERVER, proxy.url)
  await ctx.fill({ role: "textbox", name: "Username" }, "demo")
  await ctx.press({ role: "button", name: "Sign in" })
  await ctx.until(async () => (await status(ctx))?.authenticated === true, "the session to be signed in", 30000)
}

async function checkRetryScreen(ctx, name) {
  await ctx.find(REASON, { timeout: 15000 })
  ctx.check(!(await ctx.exists(COVER)), "the loading cover is gone")
  await ctx.find(RETRY)
  await ctx.find(SIGN_OUT)
  const text = await ctx.until(async () => {
    const value = await pageText(ctx)
    return /Trying again in \d+ s|Trying again now/.test(value) ? value : null
  }, "when the next attempt happens")
  ctx.check(!/os error|io:|Peer disconnected|serde|HTTP \d/i.test(text), "no raw transport or decoder text on screen")
  const body = await status(ctx)
  ctx.check(body.bootstrap?.state === "retrying" && body.bootstrap.reason === "unreachable", "/api/status reports bootstrap retrying, unreachable")
  await ctx.snapshot(name)
  await ctx.screenshot(name)
}

export default async function bootstrapRetry(ctx) {
  const proxy = await ctx.proxy({ upstream: DEMO, fail: bootstrapPage })
  await signInThrough(ctx, proxy)
  await ctx.until(() => proxy.stats.failed > 0, "the proxy to drop a catalog page", 30000)
  const retrying = await ctx.until(async () => {
    const body = await status(ctx)
    return body?.syncing === false ? body : null
  }, "the sync worker to finish the failed cycle")
  ctx.writeEvidence("01-status-retrying.json", retrying)
  ctx.step(`loading cover shown: ${await ctx.exists(COVER)}; buttons on screen: ${await ctx.count({ role: "button" })}`)
  await ctx.snapshot("01-after-failed-catalog-page")
  await ctx.screenshot("01-after-failed-catalog-page")

  await checkRetryScreen(ctx, "02-retry-screen")
  ctx.check(await ctx.windowShown(), "the main window is shown")

  await ctx.relaunch()
  await checkRetryScreen(ctx, "03-retry-screen-after-relaunch")
  await ctx.until(() => ctx.windowShown(), "the main window to be shown after relaunch")

  await ctx.press(SIGN_OUT)
  await ctx.find(SERVER, { timeout: 30000 })
  ctx.check((await status(ctx)).authenticated === false, "Sign out from the retry screen signs out")
  await ctx.screenshot("04-signed-out")

  await signInThrough(ctx, proxy)
  // The new attempt needs its request retries (about 2 s) before it can fail.
  ctx.check((await status(ctx)).bootstrap?.state === "loading", "a new sign-in starts loading, without the previous session's failure")
  await ctx.find(REASON, { timeout: 30000 })
  // The countdown reaching 0 looks the same as a pressed attempt, so press with
  // the automatic retry at least 2 s away and prove the proxy saw the pressed
  // attempt before it was due.
  const due = await ctx.until(async () => {
    const body = await status(ctx)
    const retryAt = body?.bootstrap?.state === "retrying" ? body.bootstrap.retryAt : 0
    return retryAt - Date.now() / 1000 >= 2 ? retryAt : null
  }, "an automatic retry at least 2 s away", 60000)
  const failedBefore = proxy.stats.failed
  await ctx.press(RETRY)
  await ctx.until(() => proxy.stats.failed > failedBefore, "the pressed attempt to reach the server", 5000)
  const reached = Date.now() / 1000
  ctx.check(reached < due, `the pressed attempt reached the server ${(due - reached).toFixed(1)} s before the automatic retry was due`)
  await ctx.until(async () => (await pageText(ctx)).includes("Trying again now") && (await ctx.state(RETRY)).disabled === true, "Retry now to start an attempt while the server is still down", 5000)
  await ctx.screenshot("05-retry-attempt-running")
  await ctx.until(async () => /Trying again in \d+ s/.test(await pageText(ctx)) && (await ctx.state(RETRY)).disabled !== true, "the retry screen to return after the attempt fails")
  ctx.check(await ctx.exists(REASON), "the failed pressed attempt kept the reason on screen")

  // Only a pressed Retry now explains Home appearing well before the
  // automatic attempt, so wait until that attempt is far enough away.
  const scheduled = await ctx.until(async () => {
    const body = await status(ctx)
    const retryAt = body?.bootstrap?.state === "retrying" ? body.bootstrap.retryAt : 0
    return retryAt - Date.now() / 1000 >= 15 ? retryAt : null
  }, "an automatic retry at least 15 s away", 90000)
  proxy.heal()
  const pressed = Date.now()
  await ctx.press(RETRY)
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 30000 })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  const seconds = (Date.now() - pressed) / 1000
  ctx.check(Date.now() / 1000 < scheduled, `Home appeared ${seconds.toFixed(1)} s after Retry now, before the automatic retry`)
  ctx.check((await status(ctx)).bootstrap?.state === "ready", "/api/status reports bootstrap ready")
  ctx.step(`proxy forwarded ${proxy.stats.forwarded} requests and dropped ${proxy.stats.failed}`)
  await ctx.snapshot("06-home")
  await ctx.screenshot("06-home")
}
