import { isDeepStrictEqual } from "node:util"
import { readFileSync } from "node:fs"

const LEGACY_FILES = ["config.json", "input.json"]
const MPC_HC_FIELDS = ["player_backend", "mpchc_path"]
const saveBarStatus = (ctx) => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)
const toasts = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll("[data-sonner-toast]")].map((e) => e.textContent.trim()).join(" | ") || null`)

function expectations(config, input) {
  const external = config.player_backend === "mpv" || (config.player_backend === undefined && typeof config.mpv_path === "string")
  return {
    server: config.jellyfin_url ?? null,
    backend: external ? { id: "mpv", label: "External mpv" } : { id: "libmpv", label: "Built-in player" },
    fullscreen: config.default_fullscreen === "windowed" ? "Windowed" : "Fullscreen",
    markWatchedNext: input === null ? "w" : input.bindings.mark_watched_next.trim(),
    carried: Object.fromEntries(Object.entries(config).filter(([key]) => !MPC_HC_FIELDS.includes(key) && key !== "default_fullscreen")),
  }
}

export default async function upgrade016(ctx) {
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }

  try {
    await steps(ctx, report, record)
  } finally {
    await ctx.writeEvidence("upgrade-0-1-6.json", JSON.stringify(report, null, 2))
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function steps(ctx, report, record) {
  const onDisk = (name) => readFileSync(ctx.configFile(name), "utf8")
  const seeded = Object.fromEntries(LEGACY_FILES.map((name) => [name, ctx.configStat(name) ? onDisk(name) : null]))
  if (seeded["config.json"] === null) throw new Error("seed the profile with 0.1.6's config.json")
  const config = JSON.parse(seeded["config.json"])
  const input = seeded["input.json"] === null ? null : JSON.parse(seeded["input.json"])
  const expected = expectations(config, input)
  report.seeded = seeded
  report.expected = expected
  record(ctx.configStat("settings.json") === null, "the profile starts without settings.json, as a 0.1.6 install")

  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  const server = await ctx.state({ role: "textbox", name: "Server" })
  if (expected.server) record(server.value === expected.server, "the sign-in screen is prefilled with the 0.1.6 server", server.value)
  await ctx.screenshot("01-sign-in")

  await ctx.press({ role: "link", name: "Player and app settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "Player settings while signed out")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const imported = (await ctx.api("/api/settings")).body
  report.settingsApiImported = imported
  record(imported.capabilities.libmpv === true, "the Built-in player is available here (libmpv loads)", imported.capabilities.libmpv)
  const backend = await ctx.state({ role: "combobox", name: "Player backend" })
  record(backend.value === expected.backend.label, `the Player shelf shows ${expected.backend.label}`, backend.value)
  const fullscreen = await ctx.state({ role: "combobox", name: "Default fullscreen" })
  record(fullscreen.value === expected.fullscreen, "the Player shelf shows the 0.1.6 Default fullscreen", fullscreen.value)
  await ctx.snapshot("02-player-imported")
  await ctx.screenshot("02-player-imported")
  record(ctx.configStat("settings.json") === null, "opening the settings writes nothing before Save")

  const edited = fullscreen.value === "Windowed" ? "Fullscreen" : "Windowed"
  await ctx.choose({ name: "Default fullscreen" }, edited)
  await ctx.press({ role: "button", name: "Save" })
  report.saveToast = await ctx.until(() => toasts(ctx), "a save toast")
  record(report.saveToast.includes("Player settings saved"), "Save reports success", report.saveToast)
  await ctx.screenshot("03-player-saved")
  const clean = (await ctx.state({ role: "button", name: "Save" })).disabled === true
  record(clean, "the shelf is clean after Save", await saveBarStatus(ctx))
  if (!clean) await ctx.press({ role: "button", name: "Discard" })

  const raw = ctx.configStat("settings.json") ? onDisk("settings.json") : null
  report.settingsJson = raw
  record(raw !== null, "Save wrote settings.json")
  const saved = raw === null ? {} : JSON.parse(raw)
  const savedBackend = saved.player_backend ?? (typeof saved.mpv_path === "string" ? "mpv" : "libmpv")
  record(savedBackend === expected.backend.id, `settings.json selects ${expected.backend.label}`, saved.player_backend ?? null)
  record(raw !== null && !/mpc/i.test(raw), "settings.json holds nothing about MPC-HC")
  for (const [key, value] of Object.entries(expected.carried)) {
    record(isDeepStrictEqual(saved[key], value), `settings.json keeps the 0.1.6 ${key}`, saved[key] ?? null)
  }
  record(saved.mark_watched_next === expected.markWatchedNext, `settings.json keeps the 0.1.6 mark-watched key${input === null ? " (0.1.6's default, no input.json)" : ""}`, saved.mark_watched_next ?? null)
  record((saved.default_fullscreen ?? "fullscreen") === edited.toLowerCase(), `settings.json takes the shelf's change to ${edited}`, saved.default_fullscreen ?? null)
  for (const name of LEGACY_FILES) {
    if (seeded[name] !== null) record(onDisk(name) === seeded[name], `the 0.1.6 ${name} is left byte for byte`)
  }

  await ctx.relaunch()
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen after relaunch", 30000)
  const serverAgain = await ctx.state({ role: "textbox", name: "Server" })
  if (expected.server) record(serverAgain.value === expected.server, "after a relaunch the sign-in screen is still prefilled", serverAgain.value)
  await ctx.press({ role: "link", name: "Player and app settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "Player settings after relaunch")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const backendAgain = await ctx.state({ role: "combobox", name: "Player backend" })
  record(backendAgain.value === expected.backend.label, `after a relaunch the Player shelf still shows ${expected.backend.label}`, backendAgain.value)
  const fullscreenAgain = await ctx.state({ role: "combobox", name: "Default fullscreen" })
  record(fullscreenAgain.value === edited, "after a relaunch the saved change is read from settings.json", fullscreenAgain.value)
  report.settingsApiRelaunched = (await ctx.api("/api/settings")).body
  await ctx.screenshot("04-player-relaunched")
}
