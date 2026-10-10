// The sign-in screen against Jellyfin's public demo server: the --url
// prefill, the Quick Connect probe and code (never approved), sign-in, the
// user menu, and sign-out from Home with the remembered server.
//   just verify sign-in --url https://demo.jellyfin.org/stable
const DEMO = "https://demo.jellyfin.org/stable"
const SERVER = { role: "textbox", name: "Server" }
// role=status takes no accessible name from its content, so read its text.
const quickConnectHelp = (ctx) => ctx.app.evaluate(`document.getElementById("quick-connect-help")?.textContent.trim() ?? null`)
const pageText = (ctx) => ctx.app.evaluate("document.body.innerText")

export default async function signIn(ctx) {
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  const server = await ctx.find(SERVER)
  ctx.check(server.properties.value === DEMO, "--url prefills the Server field (pass --url to the session)")
  ctx.check(ctx.readConfig("accounts.json") === null, "no accounts.json before sign-in")

  // A prefilled or remembered server is probed at once; a typed one on blur.
  const help = await ctx.until(async () => {
    const text = await quickConnectHelp(ctx)
    return text && !text.startsWith("Checking") ? text : null
  }, "the Quick Connect probe to finish", 30000)
  ctx.step(`Quick Connect help: ${help}`)
  await ctx.snapshot("01-signed-out")
  await ctx.screenshot("01-signed-out")
  if (!(await ctx.state({ role: "button", name: "Use Quick Connect" })).disabled) {
    await ctx.press({ role: "button", name: "Use Quick Connect" })
    await ctx.until(async () => (await pageText(ctx)).includes("Waiting for approval"), "a Quick Connect code waiting for approval", 20000)
    await ctx.screenshot("02-quick-connect-code")
    // Any edit of the server address drops the pending code.
    await ctx.fill(SERVER, `${DEMO}/`)
    await ctx.fill(SERVER, DEMO)
    await ctx.until(async () => !(await pageText(ctx)).includes("Waiting for approval"), "the code to go after editing the server")
  } else {
    ctx.step("Quick Connect is not available on the server; skipped the code")
  }

  await ctx.signIn({ username: "demo" })
  const status = await ctx.api("/api/status")
  ctx.check(status.body?.authenticated === true && status.body?.userName === "demo", "/api/status reports user demo")
  await ctx.until(() => ctx.readConfig("accounts.json")?.accounts?.length === 1, "accounts.json to hold the account")

  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.press({ role: "button", name: "D demo demo.jellyfin.org" })
  await ctx.find({ role: "menuitem", name: "Sync library" })
  await ctx.snapshot("03-user-menu")
  await ctx.press({ role: "menuitem", name: "Sign out" })

  const after = await ctx.find(SERVER, { timeout: 30000 })
  ctx.check(after.properties.value === DEMO, "the remembered server is prefilled after sign-out")
  // Signing out from Home (path /) also resets the window title.
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in title")
  ctx.check((await ctx.api("/api/status")).body?.authenticated === false, "/api/status reports signed out")
  ctx.check(ctx.readConfig("accounts.json")?.accounts?.length === 1, "accounts.json keeps the account after sign-out")
  ctx.check(ctx.readConfig("settings.json")?.jellyfin_url?.startsWith("https://demo.jellyfin.org"), "settings.json remembers jellyfin_url")
  await ctx.screenshot("04-signed-out-again")
}
