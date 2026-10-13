// Signs in to your own Jellyfin server with Quick Connect and caches the
// session for `--account` runs. You approve the code in Jellyfin; nobody
// types a password. Run it through its recipe, which allows ten minutes:
//   just verify-login https://jellyfin.example.com
export default async function login(ctx) {
  await ctx.quickConnect()
  ctx.saveAccount()
  await ctx.screenshot("signed-in")
}
