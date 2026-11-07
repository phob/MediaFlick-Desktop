// Run with: just verify startup-failure --services-down --seed .claude/skills/verify-mediaflick/seeds/newer-accounts
// accounts.json from a newer build makes the app's services refuse to open, so every API call answers 503,
// including the page's window-ready request. The page then falls back to its window-ready beacon,
// and the hidden main window must still be shown.
export default async function startupFailure(ctx) {
  await ctx.until(() => /unsupported account configuration version/.test(ctx.appLog()), "app.log to report the newer accounts.json", 30000)
  const status = await ctx.api("/api/status")
  ctx.check(status.status === 503, `/api/status answers 503 while services are down (got ${status.status})`)
  await ctx.until(() => ctx.windowShown(), "the main window to be shown", 30000)
  const seeded = ctx.readConfig("accounts.json")
  ctx.check(seeded?.version === 999, "the newer accounts.json is left as it was")
  await ctx.snapshot("services-down")
  await ctx.screenshot("services-down")
  ctx.writeEvidence("startup-failure.json", { status: status.status, windowShown: await ctx.windowShown(), accountsVersion: seeded?.version })
}
