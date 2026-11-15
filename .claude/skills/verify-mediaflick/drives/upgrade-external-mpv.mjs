// A MediaFlick 0.1.6 user of external mpv after the update: their config.json
// names the mpv executable and, as 0.1.6 saved its default player, no
// player_backend (v0.1.6:src/preferences/model.rs skips a default field). The
// operator's expectation: they still play through external mpv.
//
// Seed the profile with that config.json and run on your own server (the app
// starts signed in from the cached session). Closes the one-time What's new
// the 0.1.6 profile brings, checks the imported player before anything is
// saved, then runs the `playback` drive unchanged while it watches for an mpv
// process of its own outside the app: Play, seek, pause, stop, Resume and the
// startup seek rules, on the external player. Seed `default_fullscreen:
// "windowed"`, since fullscreen external mpv freezes CDP mouse input.
//   just verify upgrade-external-mpv --account https://jellyfin.example.com --seed <dir with config.json> --log-level "debug,jellyfin.playstate=trace,playback=trace"
// Writes upgrade-external-mpv.json; the playback drive writes playback.json.
import { readFileSync } from "node:fs"
import { listProcesses } from "../scripts/platform.mjs"
import playback from "./playback.mjs"

const DIALOG = { role: "dialog", name: "What's new since MediaFlick 0.1.6" }

export default async function upgradeExternalMpv(ctx) {
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }
  try {
    await steps(ctx, report, record)
  } finally {
    await ctx.writeEvidence("upgrade-external-mpv.json", JSON.stringify(report, null, 2))
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function steps(ctx, report, record) {
  if (!ctx.account || ctx.account.signedOut) throw new Error("run with --account SERVER (signed in)")
  const legacy = ctx.configStat("config.json") ? readFileSync(ctx.configFile("config.json"), "utf8") : null
  if (legacy === null) throw new Error("seed the profile with 0.1.6's config.json")
  const config = JSON.parse(legacy)
  report.config = config
  if (typeof config.mpv_path !== "string" || config.player_backend !== undefined) throw new Error("the seed must name mpv_path and, as 0.1.6 saved its default player, no player_backend")
  record(ctx.configStat("settings.json") === null, "the profile starts without settings.json, as a 0.1.6 install")

  await ctx.find(DIALOG, { timeout: 60000 })
  await ctx.screenshot("01-whats-new")
  await ctx.press({ role: "button", name: "Got it", within: DIALOG })
  await ctx.until(async () => (await ctx.count(DIALOG)) === 0, "What's new to close")

  const { client } = (await ctx.api("/api/settings")).body
  report.player = client.player
  record(client.player.playerBackend === "mpv", "the app plays with external mpv", client.player.playerBackend)
  record(client.player.mpvPath === config.mpv_path, "with the mpv executable 0.1.6 saved", client.player.mpvPath)

  // An mpv process outside the app is what external mpv means; the Built-in
  // player runs inside the app's own process.
  const seen = new Map()
  const sample = () => {
    for (const row of listProcesses()) if (row.name === "mpv" && row.cmd.includes("--input-ipc-server")) seen.set(row.pid, row.cmd)
  }
  const watcher = setInterval(sample, 500)
  try {
    await playback(ctx)
  } finally {
    clearInterval(watcher)
  }
  report.mpvProcesses = [...seen].map(([pid, cmd]) => ({ pid, cmd }))
  record(seen.size > 0 && [...seen.values()].every((cmd) => cmd.startsWith(config.mpv_path)), `playback ran ${config.mpv_path} as its own process`, report.mpvProcesses.length)
  const started = ctx.appLog().match(/starting idle mpv runtime runtime=(\w+) path=(\S+)/)
  record(started?.[1] === "External" && started[2] === config.mpv_path, "the app log names the external runtime", started?.[0] ?? null)
  await ctx.screenshot("99-after-playback")
}
