// The sign-in screen against Jellyfin's public demo server: the --url
// prefill, the Quick Connect probe while typing, the code (never approved) and
// its copy button, sign-in, the user menu, and sign-out from Home with the
// remembered server.
//   just verify sign-in --url https://demo.jellyfin.org/stable
const DEMO = "https://demo.jellyfin.org/stable"
const SERVER = { role: "textbox", name: "Server" }
// role=status takes no accessible name from its content, so read its text.
const quickConnectHelp = (ctx) => ctx.app.evaluate(`document.getElementById("quick-connect-help")?.textContent.trim() ?? null`)
const pageText = (ctx) => ctx.app.evaluate("document.body.innerText")
const serverFocused = (ctx) => ctx.app.evaluate(`document.activeElement?.id === "server"`)
const USE_QUICK_CONNECT = { role: "button", name: "Use Quick Connect" }

export default async function signIn(ctx) {
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  const server = await ctx.find(SERVER)
  ctx.check(server.properties.value === DEMO, "--url prefills the Server field (pass --url to the session)")
  ctx.check(ctx.readConfig("accounts.json") === null, "no accounts.json before sign-in")

  // A prefilled or remembered server is probed at once.
  const help = await ctx.until(async () => {
    const text = await quickConnectHelp(ctx)
    return text && !text.startsWith("Checking") ? text : null
  }, "the Quick Connect probe to finish", 30000)
  ctx.step(`Quick Connect help: ${help}`)
  await ctx.snapshot("01-signed-out")
  await ctx.screenshot("01-signed-out")

  // A typed address is probed while the field keeps focus; nothing below may blur it.
  // DEMO itself was settled by the prefill and would answer from the cache at
  // once, so the typed address differs from it to exercise the debounce.
  const typed = `${DEMO}/`
  const finishHelp = "Finish the server address to check Quick Connect availability."
  await ctx.fill(SERVER, "https://demo")
  await ctx.until(async () => (await quickConnectHelp(ctx)) === finishHelp, "the incomplete-address help")
  ctx.check((await ctx.state(USE_QUICK_CONNECT)).disabled === true, "Use Quick Connect stays disabled for https://demo")
  ctx.check(await serverFocused(ctx), "the Server field has focus after typing https://demo")

  await ctx.fill(SERVER, typed)
  const nextHelp = await ctx.until(async () => {
    const text = await quickConnectHelp(ctx)
    return text !== finishHelp ? text : null
  }, "the help to change after the full address", 2000)
  ctx.check(nextHelp === "Checking Quick Connect availability…", `the help reads Checking while the probe waits (read ${JSON.stringify(nextHelp)})`)
  await ctx.until(async () => !(await ctx.state(USE_QUICK_CONNECT)).disabled, "Use Quick Connect enabled without leaving the Server field", 30000)
  ctx.check(await serverFocused(ctx), "the Server field still has focus with Use Quick Connect enabled")
  ctx.step(`Quick Connect help: ${await quickConnectHelp(ctx)}`)
  await ctx.screenshot("02-probed-while-typing")

  await ctx.press(USE_QUICK_CONNECT)
  await ctx.until(async () => (await pageText(ctx)).includes("Waiting for approval"), "a Quick Connect code waiting for approval", 20000)
  const code = await ctx.app.evaluate(`document.querySelector('[aria-label="Copy code"]')?.previousElementSibling?.textContent ?? null`)
  ctx.check(typeof code === "string" && code.length > 0, `a Quick Connect code is shown (${code})`)
  // readText needs the permission; the app itself only writes.
  await ctx.app.send("Browser.grantPermissions", { permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"], origin: "mediaflick-desktop://app" })
  await ctx.press({ role: "button", name: "Copy code" })
  await ctx.find({ role: "button", name: "Copied" }, { timeout: 5000 })
  ctx.step("the copy button reads Copied")
  await ctx.screenshot("03-quick-connect-code-copied")
  const clipboard = await ctx.app.evaluate("navigator.clipboard.readText()")
  ctx.writeEvidence("quick-connect-clipboard.json", { code, clipboard })
  ctx.check(clipboard === code, `the clipboard holds the displayed code (${JSON.stringify(clipboard)} === ${JSON.stringify(code)})`)
  await ctx.find({ role: "button", name: "Copy code" }, { timeout: 5000 })
  ctx.step("the copy button reads Copy code again")

  // Any edit of the server address drops the pending code.
  await ctx.fill(SERVER, DEMO)
  await ctx.until(async () => !(await pageText(ctx)).includes("Waiting for approval"), "the code to go after editing the server")

  await ctx.signIn({ username: "demo" })
  const status = await ctx.api("/api/status")
  ctx.check(status.body?.authenticated === true && status.body?.userName === "demo", "/api/status reports user demo")
  await ctx.until(() => ctx.readConfig("accounts.json")?.accounts?.length === 1, "accounts.json to hold the account")

  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.press({ role: "button", name: "D demo demo.jellyfin.org" })
  await ctx.find({ role: "menuitem", name: "Sync library" })
  await ctx.snapshot("04-user-menu")
  await ctx.press({ role: "menuitem", name: "Sign out" })

  await ctx.find(SERVER, { timeout: 30000 })
  // Sign-out resets the settings query, so the field can render empty for a moment.
  await ctx.until(async () => (await ctx.find(SERVER)).properties.value === DEMO, "the remembered server prefilled after sign-out")
  // Signing out from Home (path /) also resets the window title.
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in title")
  ctx.check((await ctx.api("/api/status")).body?.authenticated === false, "/api/status reports signed out")
  ctx.check(ctx.readConfig("accounts.json")?.accounts?.length === 1, "accounts.json keeps the account after sign-out")
  ctx.check(ctx.readConfig("settings.json")?.jellyfin_url?.startsWith("https://demo.jellyfin.org"), "settings.json remembers jellyfin_url")
  await ctx.screenshot("05-signed-out-again")
}
