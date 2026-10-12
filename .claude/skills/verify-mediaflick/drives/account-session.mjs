// Sign-in, sign-out and an expired session against your own server, with no
// password: the harness approves each Quick Connect code as the cached
// account, so the app holds its own token and may sign it out.
//   just verify account-session --account https://jellyfin.example.com --signed-out
// Proves that Sign out revokes the token on the server, and that a token
// revoked from outside sends the app back to the sign-in screen as expired.
const SERVER = { role: "textbox", name: "Server" }

const status = async (ctx) => (await ctx.api("/api/status")).body
const userMenu = (ctx, user) => ({ role: "button", name: `${user[0].toUpperCase()} ${user} ${new URL(ctx.account.server).host}` })

export default async function accountSession(ctx) {
  const user = ctx.account.userName
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  ctx.check((await ctx.find(SERVER)).properties.value === ctx.account.server, "the Server field holds the account's server")
  await ctx.screenshot("01-signed-out")

  // Sign in: Quick Connect, approved from outside the app.
  await ctx.signIn({})
  const signedIn = await status(ctx)
  ctx.check(signedIn.authenticated === true && signedIn.userName === user, `/api/status reports ${user}`)
  await ctx.until(() => /authenticated against Jellyfin/.test(ctx.appLog()), "app.log: authenticated against Jellyfin")
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
  await ctx.screenshot("02-signed-in")

  // Sign out revokes the app's token on the server.
  const first = ctx.appSession()
  ctx.check((await first.status()) === 200, "the app's token is valid while signed in")
  await ctx.press(userMenu(ctx, user))
  await ctx.press({ role: "menuitem", name: "Sign out" })
  await ctx.find(SERVER, { timeout: 30000 })
  const out = await status(ctx)
  ctx.check(out.authenticated === false && out.expired === false && out.userName === null, "/api/status reports signed out")
  ctx.check((await first.status()) === 401, "Sign out revoked the token on the server")
  ctx.check(!/server-side logout failed/.test(ctx.appLog()), "app.log has no server-side logout failure")
  await ctx.until(async () => (await ctx.find(SERVER)).properties.value === ctx.account.server, "the remembered server after sign-out")
  await ctx.snapshot("03-signed-out-again")

  // A token revoked elsewhere: the next server call expires the session.
  await ctx.signIn({})
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home after signing in again")
  const second = ctx.appSession()
  await second.revoke()
  ctx.check((await second.status()) === 401, "the revoked token is rejected")
  // The next server call gets 401 and expires the session. A sync still
  // running from the sign-in makes one at once; otherwise opening a library
  // page does within about a second.
  await ctx.sleep(3000)
  if ((await status(ctx)).expired !== true) {
    ctx.step("no server call yet; opening Movies")
    await ctx.hover({ css: '[data-sidebar="sidebar"]' })
    await ctx.press({ role: "link", name: "Movies" })
  }
  const expired = await ctx.until(async () => {
    const now = await status(ctx)
    return now.expired === true ? now : null
  }, "/api/status to report the session expired", 60000)
  ctx.check(expired.authenticated === false && expired.userName === user, `the expired session keeps userName ${user}`)
  await ctx.find(SERVER, { timeout: 30000 })
  ctx.check(/rejected the stored token/.test(ctx.appLog()), "app.log: rejected the stored token")
  await ctx.snapshot("04-expired")
  await ctx.screenshot("04-expired")
  ctx.writeEvidence("account-session.json", { signedIn: { userName: signedIn.userName }, signedOut: out, expired: { authenticated: expired.authenticated, expired: expired.expired, userName: expired.userName } })
}
