// Read-only preflight for verify-mediaflick: is this checkout ready to drive,
// and is anything left over? Changes nothing. Exit code 1 lists problems.
//
//   just verify-doctor

import { execFileSync } from "node:child_process"
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs"
import net from "node:net"
import path from "node:path"
import { buildDir, exe, isWindows, listProcesses, pwshPath, realProfileDirs, root } from "./platform.mjs"
import { scanRuns, userApps } from "./runs.mjs"

const problems = []
const line = (label, text) => console.log(`${label.padEnd(14)} ${text}`)

const [major] = process.versions.node.split(".").map(Number)
line("node", `${process.version} (${process.execPath})`)
if (major < 22) problems.push("Node 22+ is required (global WebSocket)")
if (isWindows && /\\WindowsApps\\/i.test(process.execPath)) problems.push("node is MSIX-packaged; its children escape the isolation job")

function newest(dir, skip = new Set(["node_modules", "dist"])) {
  let latest = 0
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (skip.has(entry.name)) continue
    const full = path.join(dir, entry.name)
    latest = Math.max(latest, entry.isDirectory() ? newest(full, skip) : statSync(full).mtimeMs)
  }
  return latest
}
if (existsSync(exe)) {
  const built = statSync(exe).mtimeMs
  const sources = Math.max(newest(path.join(root, "src")), newest(path.join(root, "ui", "src")), ...["Cargo.toml", "build.rs", "ui/package.json"].map((file) => statSync(path.join(root, file)).mtimeMs))
  line("staged app", `${path.relative(root, exe)} built ${new Date(built).toISOString()}${sources > built ? " — STALE (sources are newer; `just verify` rebuilds first)" : " — current"}`)
} else {
  line("staged app", "missing — `just verify` builds it (or run `just build`)")
}

function which(command) {
  try {
    return execFileSync(isWindows ? "where.exe" : "sh", isWindows ? [command] : ["-c", `command -v ${command}`], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).split(/\r?\n/)[0].trim()
  } catch {
    return ""
  }
}
if (isWindows) {
  line("pwsh", existsSync(pwshPath) ? pwshPath : `missing at ${pwshPath}`)
  if (!existsSync(pwshPath)) problems.push("PowerShell 7 from the MSI is required (the justfile's windows-shell and the private-desktop wrapper use it)")
} else {
  for (const tool of ["Xvfb", "setsid", "timeout"]) {
    const found = which(tool)
    line(tool, found || "missing")
    if (!found) problems.push(`${tool} is required for the Xvfb wrapper`)
  }
  line("dbus-run-session", which("dbus-run-session") || "missing (optional: the app then runs with no session bus)")
  if (existsSync(exe) && !existsSync(path.join(buildDir, "libcef.so"))) problems.push("build/libcef.so is missing; run `just build`")
}

// CDP runs over 127.0.0.1. On some machines loopback connects intermittently
// stall for seconds; every run is then slow or times out.
async function loopbackMs() {
  const server = net.createServer((socket) => socket.end())
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve))
  const started = Date.now()
  const elapsed = await new Promise((resolve) => {
    const socket = net.connect(server.address().port, "127.0.0.1", () => resolve(Date.now() - started))
    socket.on("error", () => resolve(Infinity))
    socket.setTimeout(10000, () => resolve(Infinity))
  }).finally(() => server.close())
  return elapsed
}
const loopback = Math.max(await loopbackMs(), await loopbackMs(), await loopbackMs())
line("loopback", Number.isFinite(loopback) ? `${loopback} ms (slowest of 3 connects)` : "connect failed or took over 10 s")
if (!(loopback < 1000)) problems.push("loopback connects to 127.0.0.1 are stalling; CDP runs will crawl or time out. Wait and re-run the doctor")

const processes = listProcesses()
const runs = scanRuns(processes)
for (const run of runs) {
  const what = run.live ? `live (session pid ${run.sessionPid})` : `LEFTOVER${run.processes.length ? `, processes ${run.processes.map(({ pid }) => pid).join(", ")}` : ""}`
  line("verify run", `${run.runId ?? "?"} ${what}: ${run.dir}`)
}
if (runs.some((run) => !run.live)) problems.push("leftover verify profiles or processes; run `just verify-cleanup`")
if (!runs.length) line("verify runs", "none")

const mine = userApps(processes)
line("your app", mine.length ? `running as pid ${mine.map(({ pid }) => pid).join(", ")} (never touched; runs beside verify sessions)` : "not running")
const real = realProfileDirs()
line("real profile", `${real.config} (read-only guard target)`)

const runDirs = existsSync(path.join(buildDir, "verify"))
  ? readdirSync(path.join(buildDir, "verify"))
      .map((name) => path.join(buildDir, "verify", name))
      .filter((dir) => existsSync(path.join(dir, "steps.log")))
      .sort((a, b) => statSync(path.join(b, "steps.log")).mtimeMs - statSync(path.join(a, "steps.log")).mtimeMs)
  : []
if (runDirs.length) {
  const dir = runDirs[0]
  const resultFile = path.join(dir, "result.json")
  const live = runs.some((run) => run.live && run.runId === path.basename(dir))
  if (existsSync(resultFile)) {
    const last = JSON.parse(readFileSync(resultFile, "utf8"))
    line("last run", `${last.runId}: ${last.passed ? "passed" : "FAILED"} (exit: ${last.exit}) ${path.relative(root, dir)}`)
  } else {
    // The session writes result.json last; without it the run was killed
    // (hard timeout, closed terminal) or is still going.
    line("last run", `${path.basename(dir)}: ${live ? "still running" : "INTERRUPTED (no result.json; see its steps.log)"} ${path.relative(root, dir)}`)
  }
} else {
  line("last run", "none")
}

if (problems.length) {
  console.log(`\nproblems:\n${problems.map((problem) => `  - ${problem}`).join("\n")}`)
  process.exitCode = 1
} else {
  console.log("\nready")
}
