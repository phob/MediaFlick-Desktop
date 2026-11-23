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

  const status = await ctx.until(async () => {
    const body = (await ctx.api("/api/status")).body
    return body?.authenticated === true ? body : null
  }, "the app to start signed in", 60000)
  report.status = { authenticated: status.authenticated, userName: status.userName, server: status.server ?? null }
  record(status.authenticated === true, "the app starts signed in as before going back", status.userName)
  const whatsNewQuery = await ctx.until(`(() => {
    const root = document.getElementById("root")
    const key = Object.keys(root ?? {}).find((name) => name.startsWith("__reactContainer$"))
    const stack = key ? [root[key]] : []
    const seen = new Set()
    while (stack.length) {
      const node = stack.pop()
      if (!node || seen.has(node)) continue
      seen.add(node)
      const client = node.memoizedProps?.client
      if (client && typeof client.getQueryState === "function") {
        const state = client.getQueryState(["whats-new"])
        if (!state || state.status === "pending" || state.fetchStatus !== "idle") return null
        return { status: state.status, fetchStatus: state.fetchStatus, pending: state.data?.pending ?? null }
      }
      if (node.child) stack.push(node.child)
      if (node.sibling) stack.push(node.sibling)
    }
    return null
  })()`, "the whats-new query to settle", 15000)
  await ctx.app.evaluate(`new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))`)
  report.whatsNewQuery = whatsNewQuery
  record(whatsNewQuery.status === "success" && whatsNewQuery.pending === false, "the whats-new query settled with nothing pending", whatsNewQuery)
  const dialogCount = await ctx.count(DIALOG)
  record(dialogCount === 0, "no What's new on the return")
  record((await ctx.api("/api/settings/whats-new")).body.pending === false, "/api/settings/whats-new has nothing pending")
  await ctx.screenshot("01-home")
  if (dialogCount > 0) return

  await ctx.find({ role: "link", name: "Settings" }, { timeout: 60000 })

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
