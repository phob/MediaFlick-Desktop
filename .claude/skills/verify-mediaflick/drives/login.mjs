export default async function login(ctx) {
  await ctx.quickConnect()
  ctx.saveAccount()
  await ctx.screenshot("signed-in")
}
