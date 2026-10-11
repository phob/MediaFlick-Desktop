// Summarises perf-sign-in runs: node perf-summary.mjs <out.json> <run-id>...
// Per run it reads metrics.json and the app.log request timings from the
// sign-in press on; across runs it reports median, min and max.
import { readFileSync, writeFileSync } from "node:fs"
import path from "node:path"

// Endpoints answered from local state that the UI requests during sign-in.
const LOCAL = ["/api/status", "/api/home", "/api/collections/settings", "/api/shell/window/ready", "/api/settings/browsing"]

const [out, ...runIds] = process.argv.slice(2)
if (!out || runIds.length === 0) {
  console.error("usage: node perf-summary.mjs <out.json> <run-id>...")
  process.exit(2)
}

const runs = runIds.map((id) => {
  const dir = path.join("build", "verify", id)
  const metrics = JSON.parse(readFileSync(path.join(dir, "metrics.json"), "utf8"))
  const served = readFileSync(path.join(dir, "app.log"), "utf8")
    .split("\n")
    .filter((line) => line.includes("app request served"))
    .map((line) => ({
      path: line.match(/ path=(\S+)/)[1],
      queueMs: Number(line.match(/ queue_ms=(\d+)/)[1]),
      handlerMs: Number(line.match(/ handler_ms=(\d+)/)[1]),
    }))
  const login = served.findIndex((r) => r.path === "/api/auth/login")
  if (login < 0) throw new Error(`${id}: no /api/auth/login in app.log`)
  const afterLogin = served.slice(login + 1)
  const local = afterLogin.filter((r) => LOCAL.includes(r.path))
  const errors = metrics.requests.filter((r) => r.failed || r.status === null || r.status >= 400)
  return {
    id,
    ...metrics.milestones,
    loginMs: metrics.requests.find((r) => r.path === "/api/auth/login")?.ms ?? null,
    requests: metrics.requests.length,
    errors: errors.length,
    localRequests: local.length,
    localMaxQueueMs: Math.max(0, ...local.map((r) => r.queueMs)),
    localMaxHandlerMs: Math.max(0, ...local.map((r) => r.handlerMs)),
    anyMaxQueueMs: Math.max(0, ...afterLogin.map((r) => r.queueMs)),
  }
})

const stat = (key) => {
  const xs = runs.map((r) => r[key]).filter((x) => typeof x === "number").sort((a, b) => a - b)
  return { n: xs.length, median: xs[Math.floor(xs.length / 2)], min: xs[0], max: xs.at(-1) }
}
const keys = ["sidebar", "firstCard", "viewportImages", "loginMs", "requests", "errors", "localRequests", "localMaxQueueMs", "localMaxHandlerMs", "anyMaxQueueMs"]
const summary = { runs, stats: Object.fromEntries(keys.map((k) => [k, stat(k)])) }
writeFileSync(out, JSON.stringify(summary, null, 1))
for (const k of keys) console.log(k.padEnd(18), JSON.stringify(summary.stats[k]))
