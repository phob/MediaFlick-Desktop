import { readFileSync } from "node:fs"

const GUIDE = "https://flick.media/companion/"
const DIALOG = { role: "dialog", name: "What's new since MediaFlick 0.1.6" }
const GUIDE_LINK = { role: "link", name: "Companion install guide" }
const COMPANION_NAV = { role: "link", name: "MediaFlick Companion", within: { role: "navigation", name: "Settings navigation" } }
const SETTINGS_LINKS = ["Appearance settings", "Player settings", "Companion settings", "Home settings"]
const CLAIMS = ["Jellyfin Web", "sign in once more", "MPC-HC", "built-in player", "10.11.0", "12.0.0", "audiobooks", "books"]
const COMPANION_MINIMUM = [12, 0, 0]
const TOO_OLD_LINE = "MediaFlick Companion needs Jellyfin 12.0.0 or newer."
const ADMIN_LINE = "The guide shows how to add the plugin repository to Jellyfin"
const MEMBER_LINE = "Your server administrator can add MediaFlick Companion"

const text = (ctx, selector) => ctx.app.evaluate(`document.querySelector(${JSON.stringify(selector)})?.innerText ?? null`)
const atLeast = (version, minimum) => {
  const parts = version.split(".").map(Number)
  for (let index = 0; index < minimum.length; index++) if ((parts[index] ?? 0) !== minimum[index]) return (parts[index] ?? 0) > minimum[index]
  return true
}
const guideHref = (ctx, selector) => ctx.app.evaluate(`[...document.querySelectorAll(${JSON.stringify(`${selector} a`)})].find((a) => a.textContent.trim() === "Companion install guide")?.getAttribute("href") ?? null`)

export default async function whatsNew(ctx) {
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }
  try {
    await steps(ctx, report, record)
  } finally {
    await ctx.writeEvidence("whats-new.json", JSON.stringify(report, null, 2))
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function steps(ctx, report, record) {
  const server = process.env.MEDIAFLICK_VERIFY_WHATS_NEW_SERVER || "https://demo.jellyfin.org/stable"
  const username = process.env.MEDIAFLICK_VERIFY_WHATS_NEW_USER || "demo"
  const password = process.env.MEDIAFLICK_VERIFY_WHATS_NEW_PASSWORD || ""
  const upgraded = ctx.configStat("config.json") !== null
  const legacy = upgraded ? readFileSync(ctx.configFile("config.json")) : null
  report.profile = upgraded ? "0.1.6 config.json" : "fresh"
  ctx.step(`profile: ${report.profile}`)

  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  record((await ctx.count(DIALOG)) === 0, "no What's new on the sign-in screen")
  const signedIn = upgraded ? DIALOG : undefined
  await ctx.signIn(ctx.account?.signedOut ? { signedIn } : { server, username, password, signedIn })
  if (ctx.account?.signedOut) ctx.revokeAppSessionAfterRun()

  const me = await ctx.jellyfin("/Users/Me")
  const administrator = me.Policy?.IsAdministrator === true
  const companionInstalled = await ctx.jellyfin("/MediaFlick/info").then(() => true, (error) => {
    if (error.status === 404) return false
    throw error
  })
  const system = await ctx.jellyfin("/System/Info/Public")
  const companionSupported = atLeast(system.Version, COMPANION_MINIMUM)
  const companionMissing = !companionInstalled && companionSupported
  report.server = { url: ctx.account?.server ?? server, version: system.Version, user: me.Name, administrator, companionInstalled, companionSupported }
  ctx.step(`server ${system.Version}: ${me.Name} is ${administrator ? "an administrator" : "not an administrator"}; Companion ${companionInstalled ? "installed" : companionSupported ? "missing" : "not supported"}`)
  const status = (await ctx.api("/api/status")).body
  record(status.administrator === administrator, "the app's status reports the account's administrator flag as Jellyfin does", status.administrator)

  if (upgraded) {
    const offer = administrator && companionMissing
    if (offer) await ctx.find({ role: "heading", name: "Add MediaFlick Companion" }, { timeout: 30000 })
    else if (companionMissing) await ctx.until(async () => (await text(ctx, "[role=dialog]"))?.includes(MEMBER_LINE), "the line about the server administrator", 30000)
    await ctx.snapshot("01-whats-new")
    await ctx.screenshot("01-whats-new")
    await ctx.app.evaluate(`document.querySelector("[role=dialog]").scrollTop = 1e6`)
    await ctx.screenshot("01b-whats-new-end")
    const body = await text(ctx, "[role=dialog]")
    report.dialogText = body
    for (const claim of CLAIMS) record(body.includes(claim), `the dialog mentions ${claim}`)
    for (const name of SETTINGS_LINKS) record((await ctx.count({ role: "link", name, within: DIALOG })) === 1, `the dialog links ${name}`)
    if (!offer) await ctx.until(() => ctx.api("/api/companion/info").then(({ body }) => body?.checked === true), "the Companion check", 30000)
    const offered = await ctx.count({ role: "heading", name: "Add MediaFlick Companion" })
    record(offered === (offer ? 1 : 0), offer ? "an administrator without the Companion is offered it" : companionInstalled ? "no Companion offer where the Companion is installed" : companionSupported ? "no Companion offer for this account" : "no Companion offer on a server older than Jellyfin 12.0.0", offered)
    const href = await guideHref(ctx, "[role=dialog]")
    record(href === (offer ? GUIDE : null), offer ? "the offer links the install guide" : "the dialog has no install guide link", href)
    report.dialogTextAfterCheck = await text(ctx, "[role=dialog]")
    record(report.dialogTextAfterCheck.includes(MEMBER_LINE) === (!administrator && companionMissing), "only a non-administrator whose server can add the Companion and has not reads the line about their administrator")
    record(!ctx.readConfig("settings.json")?.whats_new_seen, "settings.json does not record What's new as seen while it is open")

    if (administrator) {
      await ctx.press({ role: "link", name: "Companion settings", within: DIALOG })
      await ctx.until(async () => (await ctx.pathname()) === "/settings/integrations/companion", "the Companion settings page from the dialog")
    } else {
      await ctx.press({ role: "button", name: "Got it" })
    }
    await ctx.until(async () => (await ctx.count(DIALOG)) === 0, "the dialog to close")
    await ctx.until(() => ctx.readConfig("settings.json")?.whats_new_seen === true, "settings.json to record What's new as seen", 10000)
    record(true, "closing the dialog records whats_new_seen in settings.json")
    record((await ctx.api("/api/settings/whats-new")).body.pending === false, "/api/settings/whats-new no longer has it pending")
  } else {
    record(ctx.configStat("config.json") === null, "a fresh profile has no config.json")
  }

  await companionSettings(ctx, report, record, { administrator, companionInstalled, companionSupported, name: upgraded ? "02-companion-settings" : "01-companion-settings" })
  record((await ctx.count(DIALOG)) === 0, "no What's new over Settings")
  if (!upgraded) {
    record((await ctx.api("/api/settings/whats-new")).body.pending === false, "/api/settings/whats-new has nothing pending on a fresh profile")
    record(!ctx.readConfig("settings.json")?.whats_new_seen, "settings.json records nothing about What's new on a fresh profile")
    return
  }

  await ctx.relaunch()
  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  record((await ctx.api("/api/status")).body.authenticated === true, "the relaunched app starts signed in")
  await companionSettings(ctx, report, record, { administrator, companionInstalled, companionSupported, name: "03-after-relaunch" })
  record((await ctx.count(DIALOG)) === 0, "a second start shows no What's new")
  record(ctx.readConfig("settings.json")?.whats_new_seen === true, "settings.json still records What's new as seen")
  record(Buffer.compare(readFileSync(ctx.configFile("config.json")), legacy) === 0, "0.1.6's config.json is left byte for byte")
}

async function companionSettings(ctx, report, record, { administrator, companionInstalled, companionSupported, name }) {
  const missing = !companionInstalled && companionSupported
  if ((await ctx.pathname()) !== "/settings/integrations/companion") {
    await ctx.press({ role: "link", name: "Settings" })
    await ctx.press(COMPANION_NAV)
  }
  await ctx.find({ role: "heading", name: "Connection" }, { timeout: 30000 })
  if (!companionSupported) {
    const said = await ctx.until(async () => (await text(ctx, ".settings-page"))?.includes(TOO_OLD_LINE), "the Companion page naming Jellyfin 12.0.0", 30000).then(() => true, () => false)
    record(said, "Companion settings say the Companion needs Jellyfin 12.0.0")
  }
  if (missing) await ctx.find(GUIDE_LINK, { timeout: 30000 })
  if (missing) {
    const line = administrator ? ADMIN_LINE : MEMBER_LINE
    const shown = await ctx.until(async () => (await text(ctx, ".settings-page"))?.includes(line), `the Companion page's ${administrator ? "administrator" : "non-administrator"} wording`, 20000).then(() => true, () => false)
    record(shown, administrator ? "Companion settings tell an administrator what the guide covers" : "Companion settings point a non-administrator to their administrator")
  }
  await ctx.screenshot(name)
  const href = await guideHref(ctx, ".settings-page")
  report[name] = { guideHref: href, text: await text(ctx, ".settings-page") }
  record(href === (missing ? GUIDE : null), missing ? "Companion settings link the install guide while the Companion is missing" : companionInstalled ? "Companion settings show no install guide with the Companion installed" : "Companion settings show no install guide on a server older than Jellyfin 12.0.0", href)
}
