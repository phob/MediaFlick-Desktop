// The account's own settings shelves on your own server: Viewing (Discard,
// Save, a revisit, Restore defaults), the leave guard between shelves,
// Appearance (accent applied live), and Letterboxd (add, save, remove). Every
// write goes to the disposable profile's accounts.json; Letterboxd also
// fetches the profile's public RSS from letterboxd.com.
//   just verify account-settings --account https://jellyfin.example.com
const LETTERBOXD_USER = "davidehrlich"

const account = (ctx) => ctx.readConfig("accounts.json")?.accounts?.find((entry) => entry.userId === ctx.account.userId) ?? null
const mtime = (ctx) => ctx.configStat("accounts.json")?.mtimeMs ?? null

async function openShelf(ctx, name) {
  if (!(await ctx.exists({ role: "navigation", name: "Settings navigation" }))) await ctx.press({ role: "link", name: "Settings" })
  // Home exists twice (sidebar and shelf); the shelf link is inside Settings navigation.
  const nth = name === "Home" ? 1 : 0
  await ctx.press({ role: "link", name }, { nth })
}

export default async function accountSettings(ctx) {
  await ctx.signIn({})
  const evidence = {}

  // Viewing: a discarded draft writes nothing; a saved one lands in accounts.json.
  await openShelf(ctx, "Viewing")
  await ctx.until(async () => (await ctx.pathname()) === "/settings/viewing", "/settings/viewing")
  await ctx.find({ role: "switch", name: "Hide watched by default" }, { timeout: 20000 })
  const before = mtime(ctx)
  await ctx.press({ role: "switch", name: "Hide watched by default" })
  await ctx.press({ role: "button", name: "Discard" })
  ctx.check((await ctx.state({ role: "switch", name: "Hide watched by default" })).checked === false, "Discard puts the switch back")
  ctx.check(mtime(ctx) === before, "Discard writes nothing")
  await ctx.press({ role: "switch", name: "Hide watched by default" })
  await ctx.fill({ role: "textbox", name: "Audio languages" }, "EN, de")

  // The leave guard between shelves while the draft is dirty.
  await ctx.press({ role: "link", name: "Appearance" })
  await ctx.find({ role: "dialog", name: "Leave without saving?" })
  await ctx.press({ role: "button", name: "Keep editing" })
  ctx.check((await ctx.pathname()) === "/settings/viewing", "Keep editing stays on Viewing")

  await ctx.press({ role: "button", name: "Save" })
  const viewing = await ctx.until(() => {
    const saved = account(ctx)?.viewing
    return saved?.hideWatched === true ? saved : null
  }, "accounts.json to record the Viewing draft")
  evidence.viewing = { hideWatched: viewing.hideWatched, audioLanguages: viewing.audioLanguages }
  ctx.check(JSON.stringify(viewing.audioLanguages) === JSON.stringify(["en", "de"]), `audio languages are saved lower-cased (${JSON.stringify(viewing.audioLanguages)})`)
  await openShelf(ctx, "Appearance")
  await openShelf(ctx, "Viewing")
  ctx.check((await ctx.state({ role: "switch", name: "Hide watched by default" })).checked === true, "the saved switch holds on a revisit")
  await ctx.snapshot("01-viewing-saved")
  await ctx.press({ role: "button", name: "Restore defaults" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => account(ctx)?.viewing?.hideWatched === false, "Viewing back to defaults")

  // Appearance: the accent applies to the whole app once saved.
  await openShelf(ctx, "Appearance")
  await ctx.choose({ name: "Accent" }, "Cobalt")
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => ctx.app.evaluate(`document.documentElement.dataset.accent === "cobalt"`), "data-accent=cobalt on <html>")
  await ctx.until(() => account(ctx)?.appearance?.accent === "cobalt", "accounts.json to record the accent")
  await ctx.screenshot("02-appearance-cobalt")
  await ctx.press({ role: "button", name: "Restore defaults" })
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => !account(ctx)?.appearance?.accent, "the accent back to its default (key left out)")

  // Letterboxd: add a public profile, save, remove, save.
  await openShelf(ctx, "Letterboxd")
  await ctx.fill({ role: "textbox", name: "Letterboxd username or profile URL" }, LETTERBOXD_USER)
  await ctx.press({ role: "button", name: "Add profile" })
  await ctx.find({ role: "button", name: `Cancel adding ${LETTERBOXD_USER}` })
  await ctx.press({ role: "button", name: "Save" })
  const letterboxd = await ctx.until(() => account(ctx)?.letterboxd_profiles?.find((profile) => profile.profileKey?.toLowerCase().includes(LETTERBOXD_USER)) ?? null, "accounts.json to hold the Letterboxd profile", 30000)
  evidence.letterboxd = { displayName: letterboxd.displayName, canonicalUrl: letterboxd.canonicalUrl, verificationStatus: letterboxd.verificationStatus }
  ctx.step(`Letterboxd profile ${letterboxd.displayName}: ${letterboxd.verificationStatus}`)
  await ctx.find({ role: "switch", name: `Enable ${letterboxd.displayName}` }, { timeout: 15000 })
  await ctx.snapshot("03-letterboxd-saved")
  // Remove only marks the row until Save; Undo brings the profile back.
  await ctx.press({ role: "button", name: "Remove profile" })
  const undo = { role: "button", name: `Undo removal of ${letterboxd.displayName}` }
  await ctx.find(undo)
  ctx.check(await ctx.app.evaluate(`document.body.innerText.includes("Will be removed when you save")`), "the removed row says it goes on Save")
  await ctx.press(undo)
  await ctx.find({ role: "switch", name: `Enable ${letterboxd.displayName}` })
  await ctx.press({ role: "button", name: "Remove profile" })
  await ctx.find(undo)
  await ctx.press({ role: "button", name: "Save" })
  await ctx.until(() => !(account(ctx)?.letterboxd_profiles?.length), "the Letterboxd profile removed from accounts.json")
  // The file lands before the save settles; closing earlier asks to confirm leaving.
  await ctx.until(async () => (await ctx.state({ role: "button", name: "Save" })).disabled === true, "the save bar to settle")
  ctx.writeEvidence("account-settings.json", evidence)
}
