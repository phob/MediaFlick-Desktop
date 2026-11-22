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
