// Home's empty states through the account's Home settings, which live in the
// disposable profile's accounts.json: only Releases left on (the demo server
// has no Companion, so it renders nothing), then nothing on, then Configure
// Home and Restore defaults + Save. Read-only against the demo server.
//   just verify home-settings
const DEMO = "https://demo.jellyfin.org/stable"
const SETTINGS_HOME = { role: "link", name: "Home" }

async function openHomeSettings(ctx) {
  await ctx.park()
  await ctx.hover({ css: '[data-sidebar="sidebar"]' })
  await ctx.press({ role: "link", name: "Settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "Settings")
  // The sidebar's Home link comes first; the second is in Settings navigation.
  await ctx.press(SETTINGS_HOME, { nth: 1 })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/home", "the Home settings shelf")
  await ctx.find({ role: "checkbox", name: "Show billboard" }, { timeout: 30000 })
}

async function saveAndGoHome(ctx) {
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "Home settings to save")
  await ctx.press(SETTINGS_HOME, { nth: 0 })
  await ctx.until(async () => (await ctx.pathname()) === "/", "Home")
}

export default async function homeSettings(ctx) {
  const results = {}
  await ctx.signIn({ server: DEMO, username: "demo" })

  await openHomeSettings(ctx)
  const shelf = await ctx.snapshot("01-home-settings")
  results.enabledByDefault = [...shelf.matchAll(/checkbox '(Show [^']*)' \[[^\]]*checked/g)].map((m) => m[1])
  ctx.check(results.enabledByDefault.includes("Show Releases"), "Releases is on by default")
  for (const name of results.enabledByDefault.filter((name) => name !== "Show Releases")) await ctx.press({ role: "checkbox", name })
  await saveAndGoHome(ctx)
  await ctx.find({ role: "heading", name: "No titles available" }, { timeout: 15000 })
  await ctx.find({ role: "link", name: "Browse movies" })
  await ctx.find({ role: "link", name: "Browse series" })
  await ctx.screenshot("02-no-titles")

  await openHomeSettings(ctx)
  await ctx.press({ role: "checkbox", name: "Show Releases" })
  await saveAndGoHome(ctx)
  await ctx.find({ role: "heading", name: "No shelves enabled" }, { timeout: 15000 })
  await ctx.screenshot("03-no-shelves")
  await ctx.press({ role: "link", name: "Configure Home" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/home", "Configure Home to open the Home settings")

  await ctx.press({ role: "button", name: "Restore defaults" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "the defaults to save")
  results.accounts = ctx.readConfig("accounts.json")
  await ctx.writeEvidence("home-settings.json", results)
}
