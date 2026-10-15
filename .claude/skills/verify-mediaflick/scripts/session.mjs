// One verification run: a disposable profile, the staged app on the isolated
// display, an in-session doctor, the drive, a coordinated exit, evidence, and
// cleanup. Start it through `just verify <drive>`, never directly: the
// isolation wrapper is what keeps the app off the user's screen.
//
//   just verify <drive> [--run-id ID] [--timeout SECONDS] [--url SERVER | --account SERVER [--signed-out] | --login SERVER] [--log-level FILTER] [--seed DIR] [--keep-profile]
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
import { checkAccount, publicAccount, readAccount, readStoredSession, redactSecret, seedLibrary, serverKey } from "./account.mjs"
import { reconcileFavorites } from "./favorites.mjs"
import { createHarness, stepLogger } from "./harness.mjs"
import { profileName, userApps } from "./runs.mjs"
import {
  appEnvironment,
  assertIsolated,
  buildDir,
  exe,
  isAppProcess,
  isLinux,
  killPids,
  listProcesses,
  mainWindowShown,
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
    account: { type: "string" },
    login: { type: "string" },
    "signed-out": { type: "boolean", default: false },
    "log-level": { type: "string", default: "debug" },
    seed: { type: "string" },
    "keep-profile": { type: "boolean", default: false },
  },
})
const [driveArg] = positionals
if (!driveArg) throw new Error("usage: just verify <drive> [--run-id ID] [--timeout SECONDS] [--url SERVER | --account SERVER [--signed-out] | --login SERVER] [--log-level FILTER] [--seed DIR] [--keep-profile]")
const drivePath = /[\\/]|\.mjs$/.test(driveArg) ? path.resolve(driveArg) : path.join(import.meta.dirname, "..", "drives", `${driveArg}.mjs`)
if (!existsSync(drivePath)) throw new Error(`no drive at ${drivePath}`)
const driveName = path.basename(drivePath, ".mjs")
const stamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15)
const runId = options["run-id"] ?? `${driveName}-${stamp}`
if (!/^[A-Za-z0-9._-]{1,80}$/.test(runId)) throw new Error("run id may only use letters, digits, '.', '_' and '-'")
const timeoutMs = Number(options.timeout) * 1000
if (!(timeoutMs > 0 && timeoutMs <= 3600_000)) throw new Error("--timeout must be 1..3600 seconds")

const isolation = assertIsolated()

if ([options.url, options.account, options.login].filter(Boolean).length > 1) throw new Error("pass only one of --url, --account and --login")
const login = options.login ? serverKey(options.login).normalized : null
const account = options.account ? readAccount(options.account) : null
if (options.account && !account) throw new Error(`no cached session for ${serverKey(options.account).normalized}; run \`just verify-login ${serverKey(options.account).normalized}\` first`)
if (options["signed-out"] && !account) throw new Error("--signed-out needs --account SERVER")
const signedOut = Boolean(account && options["signed-out"])
const accountCheck = account ? await checkAccount(account) : null
const secrets = new Set(account ? [account.token] : [])
let signOutBlocked = false

const evidence = path.join(buildDir, "verify", runId)
if (existsSync(evidence)) throw new Error(`${path.relative(root, evidence)} already exists; pick a new --run-id`)
mkdirSync(evidence, { recursive: true })
const log = stepLogger(path.join(evidence, "steps.log"))
const writeJson = (name, value) => writeFileSync(path.join(evidence, name), `${JSON.stringify(value, null, 2)}\n`)
const started = Date.now()
const result = { runId, drive: path.relative(root, drivePath), passed: false, error: null, exit: null, evidence: path.relative(root, evidence) }
// A promise a drive forgot to await must fail the drive and still reach
// teardown and the after-run tasks, not crash the process past them.
let unhandledError = null
let failUnhandled
const unhandled = new Promise((_, reject) => {
  failUnhandled = reject
})
unhandled.catch(() => {})
process.on("unhandledRejection", (reason) => {
  unhandledError ??= reason instanceof Error ? reason : new Error(`non-Error rejection: ${String(reason)}`)
  log(`unhandled rejection: ${unhandledError.message}`)
  failUnhandled(unhandledError)
})
log(`isolation ok: ${JSON.stringify(isolation)}`)
if (account) {
  writeJson("account.json", { mode: signedOut ? "account-signed-out" : "account", ...publicAccount(account), check: accountCheck })
  log(`cached session for ${account.userName} on ${account.server} is valid (HTTP ${accountCheck.status})`)
  await reconcileFavorites(account, log).catch((error) => {
    log(`FAILED before launch: ${error.message}`)
    throw error
  })
}

if (!existsSync(exe)) throw new Error(`${exe} is missing; \`just verify\` builds it, or run \`just build\``)

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

const profile = path.join(profileParent, profileName(runId, process.pid, randomBytes(3).toString("hex")))
const { env, dirs, configDir, dataDir } = appEnvironment(profile)
// Sockets in TMPDIR must fit the 107-byte Unix path limit: CEF dies with
// SIGTRAP on a long SingletonSocket path, and the libmpv shell cannot reach
// mpv's IPC socket (mediaflick-desktop-<pid>-<nanos>-<n>.sock, about 55 bytes).
if (isLinux && Buffer.byteLength(env.TMPDIR ?? "") + 56 > 107) {
  throw new Error(`TMPDIR ${env.TMPDIR} is too long for the app's Unix sockets; run from a shorter TMPDIR`)
}
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
if (account && !signedOut) {
  seedLibrary(dataDir, account)
  log(`seeded library.db with the cached session for ${account.userName}`)
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
  const args = ["--remote-debugging-port", String(port), "--log-level", options["log-level"]]
  const url = options.url ?? login ?? (signedOut ? account.serverUrl : null)
  if (url) args.push("--url", url)
  child = spawn(exe, args, { cwd: buildDir, env, stdio: "ignore" })
  const exited = new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })))
  child.exited = exited
  writeJson("pids.json", { session: process.pid, app: child.pid, port, profile })
  log(`launched pid ${child.pid} on CDP port ${port}`)
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
  if ((account && !signedOut) || login) await guardSignOut()
  return port
}

// Sign out posts /api/auth/logout, and the app then revokes the token on the
// server, which would kill the cached session. CDP's Fetch domain does not see
// the app's custom-scheme requests, so the page's fetch refuses that one call
// and reports it through a binding; the harness fails the drive at once.
const SIGN_OUT_BINDING = "mediaflickVerifySignOutBlocked"
const SIGN_OUT_GUARD = `(() => {
  if (window.${SIGN_OUT_BINDING}Installed) return
  window.${SIGN_OUT_BINDING}Installed = true
  const original = window.fetch
  window.fetch = function (input, init) {
    const url = input instanceof Request ? input.url : String(input)
    if (new URL(url, location.href).pathname === "/api/auth/logout") {
      try { window.${SIGN_OUT_BINDING}(url) } catch {}
      return Promise.reject(new TypeError("verify: Sign out is blocked for a cached Jellyfin account"))
    }
    return original.apply(this, arguments)
  }
})()`

async function guardSignOut() {
  app.on("Runtime.bindingCalled", ({ name }) => {
    if (name !== SIGN_OUT_BINDING) return
    signOutBlocked = true
    log("BLOCKED Sign out: it would revoke the cached session's token on the server")
  })
  await app.send("Runtime.addBinding", { name: SIGN_OUT_BINDING })
  await app.send("Page.addScriptToEvaluateOnNewDocument", { source: SIGN_OUT_GUARD })
  await app.evaluate(SIGN_OUT_GUARD)
}

async function doctor(port, file = "doctor.json") {
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
  writeJson(file, report)
  const problems = []
  if (!href.startsWith("mediaflick-desktop://app")) problems.push(`CDP page is ${href}, not the app`)
  if (status.status !== 200) problems.push(`/api/status answered ${status.status}`)
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

async function closeApp() {
  let exit
  if (child.exitCode === null && child.signalCode === null && app) {
    log("closing the app window")
    await within(app.evaluate("window.close(), true").catch(() => {}), 5000, () => {})
    const closed = await within(child.exited.then(() => true), 20000, () => false)
    exit = closed ? "coordinated" : "forced"
  } else {
    exit = child.exitCode === null && child.signalCode === null ? "forced" : "exited-early"
  }
  app?.close()
  if (exit === "forced") {
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
  return { exit, leftovers: left.map(({ pid, name }) => ({ pid, name })) }
}

async function teardown() {
  if (!child) return
  const { exit, leftovers } = await closeApp()
  result.exit = exit
  if (leftovers.length) result.leftovers = leftovers
}

async function relaunch() {
  const { exit, leftovers } = await closeApp()
  if (exit !== "coordinated" || leftovers.length) throw new Error(`relaunch: the app exited ${exit}${leftovers.length ? ` and left ${JSON.stringify(leftovers)}` : ""}`)
  log(`relaunching on the same profile (exit: ${exit})`)
  await doctor(await launch(), "doctor-relaunch.json")
  return app
}

async function failureEvidence(harness) {
  if (!harness) return
  await within(harness.screenshot("failure"), 10000, () => log("failure screenshot timed out")).catch(() => {})
  await within(harness.snapshot("failure"), 10000, () => log("failure snapshot timed out")).catch(() => {})
}

function collectProfile() {
  const target = path.join(evidence, "profile")
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
  harness = createHarness({
    app,
    currentChild: () => child,
    evidence,
    configDir,
    dataDir,
    log,
    relaunchApp: relaunch,
    windowShown: () => mainWindowShown(child.pid),
    account: account ? { server: account.server, userName: account.userName, userId: account.userId, login: false, signedOut } : login ? { server: login, login: true } : null,
    cachedAccount: account,
    signOutBlocked: () => signOutBlocked,
    rememberSecret: (token) => secrets.add(token),
  })
  const drive = (await import(pathToFileURL(drivePath).href)).default
  if (typeof drive !== "function") throw new Error(`${result.drive} must export a default async function (ctx)`)
  log(`drive ${result.drive}`)
  await Promise.race([
    within(drive(harness), timeoutMs, () => {
      throw new Error(`drive timed out after ${timeoutMs / 1000} s`)
    }),
    unhandled,
  ])
  if (signOutBlocked) throw new Error("the drive tried to sign out of a cached Jellyfin account")
  if (login && !secrets.size) throw new Error("the login drive did not cache a session (ctx.saveAccount)")
  result.passed = true
  log("drive passed")
} catch (error) {
  result.error = error instanceof Error ? error.stack : `non-Error rejection: ${error?.type ?? String(error)}`
  log(`FAILED: ${error instanceof Error ? error.message : result.error}`)
  await failureEvidence(harness)
} finally {
  const cleanupErrors = []
  for (const [name, stop] of [["teardown", teardown], ["dispose", async () => harness?.dispose()]]) {
    try {
      await stop()
    } catch (error) {
      cleanupErrors.push(`${name} failed: ${error instanceof Error ? error.message : String(error)}`)
      log(cleanupErrors.at(-1))
    }
  }
  const afterRunErrors = (await harness?.runAfterRun()) ?? []
  if (afterRunErrors.length) cleanupErrors.push(`after-run cleanup failed: ${afterRunErrors.join("; ")}`)
  if (unhandledError && result.passed) cleanupErrors.push(`unhandled rejection: ${unhandledError.message}`)
  if (cleanupErrors.length) {
    result.passed = false
    result.error = `${result.error ? `${result.error}\n` : ""}${cleanupErrors.join("\n")}`
  }
  collectProfile()
  if (account || login) {
    try {
      const stored = readStoredSession(dataDir)
      if (stored?.token) secrets.add(stored.token)
    } catch (error) {
      log(`could not read the profile's session for the token scan: ${error.message}`)
    }
  }
  const guardAfter = fingerprint()
  const changed = Object.keys(guardBefore).filter((file) => JSON.stringify(guardBefore[file]) !== JSON.stringify(guardAfter[file]))
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
  if (secrets.size) {
    const leaked = [...secrets].flatMap((secret) => redactSecret(evidence, secret)).map((file) => path.relative(evidence, file))
    result.tokenScan = { scanned: true, leaked }
    if (leaked.length) {
      result.passed = false
      result.error = `${result.error ? `${result.error}\n` : ""}the Jellyfin token reached evidence (now redacted): ${leaked.join(", ")}`
    }
    log(leaked.length ? `token scan: redacted the token from ${leaked.join(", ")}` : "token scan ok: no evidence file holds the token")
  }
  result.durationMs = Date.now() - started
  writeJson("result.json", result)
  log(`${result.passed ? "PASSED" : "FAILED"} ${path.relative(root, evidence)} (exit: ${result.exit})`)
  // A timed-out drive is abandoned, not cancelled; its pending CDP calls or
  // timers must not keep the run alive once the result is on disk.
  process.exit(result.passed ? 0 : 1)
}
