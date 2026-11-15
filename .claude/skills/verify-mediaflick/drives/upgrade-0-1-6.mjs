// The upgrade from MediaFlick 0.1.6, whose settings live in config.json and
// whose mark-watched key lives in input.json (there is no settings.json yet).
// The seed holds those two files as 0.1.6 wrote them (to_vec_pretty of
// AppSettings in v0.1.6:src/preferences/model.rs and of MpvInputBindings in
// v0.1.6:src/players/mpv/input.rs), choosing MPC-HC, which 0.2.0 dropped, with
// an mpv path also set. The operator decided on 2026-11-15 that an MPC-HC user
// moves to the Built-in player and keeps every other setting.
//
// Proves: the sign-in screen is prefilled with the 0.1.6 server, the Player
// shelf shows Built-in player and the other 0.1.6 values, Save writes
// settings.json with player_backend libmpv, no MPC-HC and every other 0.1.6
// value, the 0.1.6 files are left byte for byte, and after a relaunch the app
// reads settings.json. Every check is recorded in upgrade-0-1-6.json and the run
// fails at the end listing each failed one. Never signs in.
//   just verify upgrade-0-1-6 --seed .claude/skills/verify-mediaflick/seeds/upgrade-0-1-6
import { readFileSync } from "node:fs"

const SEED = new URL("../seeds/upgrade-0-1-6/", import.meta.url)
const SERVER = "https://demo.jellyfin.org/stable"
const saveBarStatus = (ctx) => ctx.app.evaluate(`document.querySelector(".settings-save-bar [role=status]")?.textContent.trim() ?? null`)
const toasts = (ctx) => ctx.app.evaluate(`[...document.querySelectorAll("[data-sonner-toast]")].map((e) => e.textContent.trim()).join(" | ") || null`)

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
  const seeded = (name) => readFileSync(new URL(name, SEED), "utf8")
  const onDisk = (name) => readFileSync(ctx.configFile(name), "utf8")
  record(ctx.configStat("settings.json") === null, "the profile starts without settings.json, as a 0.1.6 install")
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  const server = await ctx.state({ role: "textbox", name: "Server" })
  record(server.value === SERVER, "the sign-in screen is prefilled with the 0.1.6 server", server.value)
  await ctx.screenshot("01-sign-in")

  await ctx.press({ role: "link", name: "Player and app settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "Player settings while signed out")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const imported = (await ctx.api("/api/settings")).body
  report.settingsApiImported = imported
  record(imported.capabilities.libmpv === true, "the Built-in player is available here (libmpv loads)", imported.capabilities.libmpv)
  const backend = await ctx.state({ role: "combobox", name: "Player backend" })
  record(backend.value === "Built-in player", "the Player shelf shows Built-in player for an MPC-HC user", backend.value)
  const fullscreen = await ctx.state({ role: "combobox", name: "Default fullscreen" })
  record(fullscreen.value === "Windowed", "the Player shelf shows the 0.1.6 Default fullscreen", fullscreen.value)
  await ctx.snapshot("02-player-imported")
  await ctx.screenshot("02-player-imported")
  record(ctx.configStat("settings.json") === null, "opening the settings writes nothing before Save")

  // The edit that makes the shelf dirty: Fullscreen once the 0.1.6 Windowed was carried over.
  const edited = fullscreen.value === "Windowed" ? "Fullscreen" : "Windowed"
  await ctx.choose({ name: "Default fullscreen" }, edited)
  await ctx.press({ role: "button", name: "Save" })
  report.saveToast = await ctx.until(() => toasts(ctx), "a save toast")
  record(report.saveToast.includes("Player settings saved"), "Save reports success", report.saveToast)
  await ctx.screenshot("03-player-saved")
  const clean = (await ctx.state({ role: "button", name: "Save" })).disabled === true
  record(clean, "the shelf is clean after Save", await saveBarStatus(ctx))
  // A failed save leaves the draft dirty, and the relaunch's close would stop at the leave dialog.
  if (!clean) await ctx.press({ role: "button", name: "Discard" })

  const raw = ctx.configStat("settings.json") ? onDisk("settings.json") : null
  report.settingsJson = raw
  record(raw !== null, "Save wrote settings.json")
  const saved = raw === null ? {} : JSON.parse(raw)
  record(saved.player_backend === "libmpv", "settings.json names the Built-in player", saved.player_backend)
  record(raw !== null && !/mpc/i.test(raw), "settings.json holds nothing about MPC-HC")
  for (const [key, expected] of [
    ["jellyfin_url", SERVER],
    ["mpv_path", "/usr/bin/mpv"],
    ["streaming_quality", "20_mbps"],
    ["show_scrollbars", true],
    ["skip_intro", "always"],
    ["skip_credits", "disabled"],
    ["mark_watched_next", "n"],
  ]) {
    record(saved[key] === expected, `settings.json keeps the 0.1.6 ${key}`, saved[key])
  }
  record(saved.default_fullscreen === (edited === "Windowed" ? "windowed" : undefined), `settings.json takes the shelf's change to ${edited}`, saved.default_fullscreen ?? null)
  for (const name of ["config.json", "input.json"]) {
    record(onDisk(name) === seeded(name), `the 0.1.6 ${name} is left byte for byte`)
  }

  await ctx.relaunch()
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen after relaunch", 30000)
  const serverAgain = await ctx.state({ role: "textbox", name: "Server" })
  record(serverAgain.value === SERVER, "after a relaunch the sign-in screen is still prefilled", serverAgain.value)
  await ctx.press({ role: "link", name: "Player and app settings" })
  await ctx.until(async () => (await ctx.pathname()) === "/settings/client/player", "Player settings after relaunch")
  await ctx.find({ role: "combobox", name: "Player backend" }, { timeout: 30000 })
  const backendAgain = await ctx.state({ role: "combobox", name: "Player backend" })
  record(backendAgain.value === "Built-in player", "after a relaunch the Player shelf still shows Built-in player", backendAgain.value)
  const fullscreenAgain = await ctx.state({ role: "combobox", name: "Default fullscreen" })
  record(fullscreenAgain.value === edited, "after a relaunch the saved change is read from settings.json", fullscreenAgain.value)
  report.settingsApiRelaunched = (await ctx.api("/api/settings")).body
  await ctx.screenshot("04-player-relaunched")
}
