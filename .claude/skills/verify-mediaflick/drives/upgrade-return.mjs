import { readFileSync } from "node:fs"

const DIALOG = { role: "dialog", name: "What's new since MediaFlick 0.1.6" }

export default async function upgradeReturn(ctx) {
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }
  try {
    await steps(ctx, report, record)
  } finally {
    await ctx.writeEvidence("upgrade-return.json", JSON.stringify(report, null, 2))
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function steps(ctx, report, record) {
  const read = (name) => (ctx.configStat(name) ? readFileSync(ctx.configFile(name), "utf8") : null)
  const settingsJson = read("settings.json")
  const configJson = read("config.json")
  if (settingsJson === null || configJson === null) throw new Error("seed a profile that has both settings.json and 0.1.6's config.json")
  const settings = JSON.parse(settingsJson)
  report.settings = settings
  report.config = JSON.parse(configJson)

  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })
  const status = (await ctx.api("/api/status")).body
  report.status = { authenticated: status.authenticated, userName: status.userName, server: status.server ?? null }
  record(status.authenticated === true, "the app starts signed in as before going back", status.userName)
  const dialog = await ctx.until(async () => (await ctx.count(DIALOG)) > 0, "What's new (it must not open)", 8000).then(() => true, () => false)
  record(!dialog, "no What's new on the return")
  record((await ctx.api("/api/settings/whats-new")).body.pending === false, "/api/settings/whats-new has nothing pending")
  await ctx.screenshot("01-home")

  await ctx.press({ role: "link", name: "Settings" })
  await ctx.press({ role: "link", name: "Player", within: { role: "navigation", name: "Settings navigation" } })
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const backend = await ctx.state({ role: "combobox", name: "Player backend" })
  const expected = (settings.player_backend ?? (typeof settings.mpv_path === "string" ? "mpv" : "libmpv")) === "mpv" ? "External mpv" : "Built-in player"
  record(backend.value === expected, `the Player shelf shows settings.json's ${expected}`, backend.value)
  const fullscreen = await ctx.state({ role: "combobox", name: "Default fullscreen" })
  record(fullscreen.value === (settings.default_fullscreen === "windowed" ? "Windowed" : "Fullscreen"), "the Player shelf shows settings.json's Default fullscreen", fullscreen.value)
  await ctx.screenshot("02-player")
  record(read("settings.json") === settingsJson, "starting leaves settings.json byte for byte")
  record(read("config.json") === configJson, "starting leaves 0.1.6's config.json byte for byte")
}
