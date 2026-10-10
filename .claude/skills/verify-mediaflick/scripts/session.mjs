// One verification run: a disposable profile, the staged app on the isolated
// display, an in-session doctor, the drive, a coordinated exit, evidence, and
// cleanup. Start it through `just verify <drive>`, never directly: the
// isolation wrapper is what keeps the app off the user's screen.
//
//   just verify <drive> [--run-id ID] [--timeout SECONDS] [--url SERVER] [--seed DIR] [--keep-profile]
//
// <drive> is a file path or the name of a file in ../drives (without .mjs).
// Evidence goes to build/verify/<run-id>/ and survives cleanup.

import { execFileSync, spawn } from "node:child_process"
import { randomBytes } from "node:crypto"
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import net from "node:net"
import path from "node:path"
import { pathToFileURL } from "node:url"
import { parseArgs } from "node:util"
import { connect, sleep } from "../../../../scripts/website/cdp.mjs"
import { createHarness, stepLogger } from "./harness.mjs"
import { profileName, userApps } from "./runs.mjs"
import {
  appEnvironment,
  assertIsolated,
  buildDir,
  exe,
  isAppProcess,
  killPids,
  listProcesses,
  profileParent,
  realProfileDirs,
  root,
} from "./platform.mjs"

const { values: options, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    "run-id": { type: "string" },
    timeout: { type: "string", default: "300" },
    url: { type: "string" },
    seed: { type: "string" },
    "keep-profile": { type: "boolean", default: false },
  },
})
const [driveArg] = positionals
if (!driveArg) throw new Error("usage: just verify <drive> [--run-id ID] [--timeout SECONDS] [--url SERVER] [--seed DIR] [--keep-profile]")
const drivePath = /[\\/]|\.mjs$/.test(driveArg) ? path.resolve(driveArg) : path.join(import.meta.dirname, "..", "drives", `${driveArg}.mjs`)
if (!existsSync(drivePath)) throw new Error(`no drive at ${drivePath}`)
const driveName = path.basename(drivePath, ".mjs")
const stamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15)
const runId = options["run-id"] ?? `${driveName}-${stamp}`
if (!/^[A-Za-z0-9._-]{1,80}$/.test(runId)) throw new Error("run id may only use letters, digits, '.', '_' and '-'")
const timeoutMs = Number(options.timeout) * 1000
if (!(timeoutMs > 0 && timeoutMs <= 3600_000)) throw new Error("--timeout must be 1..3600 seconds")

// Isolation comes first: nothing is launched before it is confirmed.
const isolation = assertIsolated()

const evidence = path.join(buildDir, "verify", runId)
if (existsSync(evidence)) throw new Error(`${path.relative(root, evidence)} already exists; pick a new --run-id`)
mkdirSync(evidence, { recursive: true })
const log = stepLogger(path.join(evidence, "steps.log"))
const writeJson = (name, value) => writeFileSync(path.join(evidence, name), `${JSON.stringify(value, null, 2)}\n`)
const started = Date.now()
const result = { runId, drive: path.relative(root, drivePath), passed: false, error: null, exit: null, evidence: path.relative(root, evidence) }
log(`isolation ok: ${JSON.stringify(isolation)}`)

if (!existsSync(exe)) throw new Error(`${exe} is missing; \`just verify\` builds it, or run \`just build\``)

// --- Read-only guard over the user's real profile. ---
const real = realProfileDirs()
const GUARDED = [
  ["config", "accounts.json"], ["config", "settings.json"], ["config", "collections.json"], ["config", "playback-preferences.json"],
  ["config", "pending-deletions.json"], ["config", "instance.json"], ["data", "library.db"],
]
const fingerprint = () => Object.fromEntries(GUARDED.map(([kind, name]) => {
  const file = path.join(real[kind], name)
  try {
    const { size, mtimeMs } = statSync(file)
    return [file, { size, mtimeMs }]
  } catch {
    return [file, null]
  }
}))
const guardBefore = fingerprint()

// --- Disposable profile. ---
const profile = path.join(profileParent, profileName(runId, process.pid, randomBytes(3).toString("hex")))
const { env, dirs, configDir, dataDir } = appEnvironment(profile)
for (const dir of dirs) mkdirSync(dir, { recursive: true })
mkdirSync(configDir, { recursive: true })
mkdirSync(dataDir, { recursive: true })
for (const realDir of Object.values(real)) {
  if ([configDir, dataDir].some((dir) => path.resolve(dir) === path.resolve(realDir))) throw new Error("disposable profile resolves to the real profile; refusing")
}
if (options.seed) {
  cpSync(path.resolve(options.seed), configDir, { recursive: true })
  log(`seeded ${configDir} from ${options.seed}`)
}
log(`profile ${profile}`)

const freePort = () => new Promise((resolve, reject) => {
  const server = net.createServer()
  server.once("error", reject)
  server.listen(0, "127.0.0.1", () => {
    const { port } = server.address()
    server.close(() => resolve(port))
  })
})

const ours = () => listProcesses().filter((row) => row.cmd.includes(profile))

// Races a promise against a deadline whose timer is cleared either way, so a
// finished run never lingers on a pending timeout.
function within(promise, ms, onTimeout) {
  let timer
  const deadline = new Promise((resolve, reject) => {
    timer = setTimeout(() => {
      try {
        resolve(onTimeout())
      } catch (error) {
        reject(error)
      }
    }, ms)
  })
  return Promise.race([promise, deadline]).finally(() => clearTimeout(timer))
}
let child
let app

async function launch() {
  const port = await freePort()
  const args = ["--remote-debugging-port", String(port), "--log-level", "debug"]
  if (options.url) args.push("--url", options.url)
  child = spawn(exe, args, { cwd: buildDir, env, stdio: "ignore" })
  const exited = new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })))
  child.exited = exited
  writeJson("pids.json", { session: process.pid, app: child.pid, port, profile })
  log(`launched pid ${child.pid} on CDP port ${port}`)
  // Loopback connects fail intermittently on some machines; the WebSocket
  // rejects with a bare ErrorEvent, so retry and keep a readable message.
  const attach = async () => {
    for (let attempt = 1; ; attempt++) {
      try {
        return await connect(port)
      } catch (error) {
        const message = error instanceof Error ? error.message : `${error?.type ?? "error"} event from the CDP socket`
        if (attempt === 3) throw new Error(`cannot attach to CDP on port ${port}: ${message}`)
        log(`CDP attach attempt ${attempt} failed (${message}); retrying`)
        await sleep(1000)
      }
    }
  }
  app = await Promise.race([
    attach(),
    exited.then(({ code, signal }) => {
      throw new Error(`app exited during startup (code ${code}, signal ${signal}); see app.log`)
    }),
  ])
  for (const domain of ["Page", "Runtime", "DOM", "Accessibility"]) await app.send(`${domain}.enable`)
  return port
}

async function doctor(port) {
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json()
  const href = await app.evaluate("location.href")
  await app.waitFor("document.readyState === 'complete' && !!document.querySelector('#root')?.childElementCount", { timeout: 30000 })
  const status = await app.evaluate("fetch('/api/status').then(async (r) => ({ status: r.status, body: await r.json() }))")
  const processes = ours()
  const report = {
    isolation,
    app: { pid: child.pid, exe, exeModified: statSync(exe).mtime.toISOString(), git: gitHead(), browser: version.Browser, href },
    port,
    status,
    profile: { root: profile, configDir, dataDir, files: readdirSync(configDir) },
    processes: processes.map(({ pid, ppid, name }) => ({ pid, ppid, name })),
  }
  writeJson("doctor.json", report)
  const problems = []
  if (!href.startsWith("mediaflick-desktop://app")) problems.push(`CDP page is ${href}, not the app`)
  if (status.status !== 200) problems.push(`/api/status answered ${status.status}`)
  // instance.json and the log appear in the profile only if the app really
  // resolved its config dir there.
  if (!existsSync(path.join(configDir, "instance.json"))) problems.push("the app did not create instance.json in the disposable config dir")
  if (!processes.some((row) => row.pid === child.pid) && !listProcesses().some((row) => row.pid === child.pid && isAppProcess(row))) problems.push(`pid ${child.pid} is not a running app process`)
  if (problems.length) throw new Error(`doctor: ${problems.join("; ")}`)
  log(`doctor ok: ${version.Browser}, ${processes.length} app processes on this profile, signed in: ${status.body?.authenticated === true}`)
}

function gitHead() {
  try {
    return execFileSync("git", ["-C", root, "rev-parse", "--short", "HEAD"], { encoding: "utf8" }).trim()
  } catch {
    return null
  }
}

// Closes the window the way the app's own close path does, so save-on-exit
// runs, then falls back to stopping exactly the processes this run started.
async function teardown() {
  if (!child) return
  if (child.exitCode === null && child.signalCode === null && app) {
    log("closing the app window")
    // A stalled CDP link must not hold teardown; the exit wait covers it.
    await within(app.evaluate("window.close(), true").catch(() => {}), 5000, () => {})
    const closed = await within(child.exited.then(() => true), 20000, () => false)
    result.exit = closed ? "coordinated" : "forced"
  } else {
    result.exit = child.exitCode === null && child.signalCode === null ? "forced" : "exited-early"
  }
  app?.close()
  if (result.exit === "forced") {
    log(`app did not exit; stopping pid ${child.pid} and its children`)
    killPids([child.pid], { tree: true })
  }
  // CEF helpers carry --user-data-dir inside this run's unique profile.
  for (let attempt = 0; attempt < 20; attempt++) {
    const left = ours()
    if (!left.length) break
    if (attempt === 10) {
      log(`stopping leftover processes ${left.map((row) => row.pid).join(", ")}`)
      killPids(left.map((row) => row.pid))
    }
    await sleep(500)
  }
  const left = ours()
  if (left.length) result.leftovers = left.map(({ pid, name }) => ({ pid, name }))
}

async function failureEvidence(harness) {
  if (!harness) return
  await within(harness.screenshot("failure"), 10000, () => log("failure screenshot timed out")).catch(() => {})
  await within(harness.snapshot("failure"), 10000, () => log("failure snapshot timed out")).catch(() => {})
}

function collectProfile() {
  const target = path.join(evidence, "profile")
  // Durable files the app wrote; CEF's cache and the catalog DB stay behind.
  if (existsSync(configDir)) cpSync(configDir, path.join(target, "config"), { recursive: true })
  for (const name of ["cef.log"]) {
    const file = path.join(dataDir, name)
    if (existsSync(file)) cpSync(file, path.join(evidence, name))
  }
  const log = path.join(configDir, "mediaflick-desktop.log")
  if (existsSync(log)) cpSync(log, path.join(evidence, "app.log"))
}

let harness
try {
  const port = await launch()
  await doctor(port)
  harness = createHarness({ app, evidence, configDir, dataDir, log })
  const drive = (await import(pathToFileURL(drivePath).href)).default
  if (typeof drive !== "function") throw new Error(`${result.drive} must export a default async function (ctx)`)
  log(`drive ${result.drive}`)
  await within(drive(harness), timeoutMs, () => {
    throw new Error(`drive timed out after ${timeoutMs / 1000} s`)
  })
  result.passed = true
  log("drive passed")
} catch (error) {
  result.error = error instanceof Error ? error.stack : `non-Error rejection: ${error?.type ?? String(error)}`
  log(`FAILED: ${error instanceof Error ? error.message : result.error}`)
  await failureEvidence(harness)
} finally {
  await teardown()
  collectProfile()
  const guardAfter = fingerprint()
  const changed = Object.keys(guardBefore).filter((file) => JSON.stringify(guardBefore[file]) !== JSON.stringify(guardAfter[file]))
  // The user's own MediaFlick may legitimately write its profile meanwhile.
  const running = userApps()
  writeJson("guard.json", { realProfile: real, changed, userAppsRunning: running.map(({ pid }) => pid), before: guardBefore, after: guardAfter })
  if (changed.length && !running.length) {
    result.passed = false
    result.error = `${result.error ? `${result.error}\n` : ""}real profile files changed during the run: ${changed.join(", ")}`
  }
  log(changed.length ? `guard: real profile files changed: ${changed.join(", ")}${running.length ? " (your own MediaFlick is running)" : ""}` : "guard ok: real profile untouched")
  if (result.leftovers) {
    result.passed = false
    result.error = `${result.error ? `${result.error}\n` : ""}processes survived cleanup: ${JSON.stringify(result.leftovers)}`
  }
  if (!options["keep-profile"]) {
    try {
      rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 })
      result.profileRemoved = true
    } catch (error) {
      result.profileRemoved = false
      log(`could not remove ${profile}: ${error.message}`)
    }
  } else {
    result.profileRemoved = false
    log(`kept profile ${profile}`)
  }
  result.durationMs = Date.now() - started
  writeJson("result.json", result)
  log(`${result.passed ? "PASSED" : "FAILED"} ${path.relative(root, evidence)} (exit: ${result.exit})`)
  // A timed-out drive is abandoned, not cancelled; its pending CDP calls or
  // timers must not keep the run alive once the result is on disk.
  process.exit(result.passed ? 0 : 1)
}
