// Launch and evidence only: the signed-out first screen of a fresh profile.
//   just verify smoke
export default async function smoke(ctx) {
  // The card title is not a heading; the window title names the screen.
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  await ctx.find({ role: "textbox", name: "Server" })
  ctx.check((await ctx.state({ role: "button", name: "Sign in" })).disabled === true, "Sign in is disabled until a server is entered")
  const status = await ctx.api("/api/status")
  ctx.check(status.status === 200 && status.body.authenticated === false, "a fresh profile starts signed out")
  ctx.check(ctx.readConfig("accounts.json") === null, "a fresh profile has no accounts.json")
  await ctx.snapshot("sign-in")
  await ctx.screenshot("sign-in")
}
