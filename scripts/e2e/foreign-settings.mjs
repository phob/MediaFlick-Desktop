// End-to-end check that settings written by another MediaFlick version
// survive this one. An older build once read a newer accounts.json (a Home
// shelf it did not know) and settings.json as damage: it moved both aside
// and started from defaults, wiping the user's preferences.
//
//   just build
//   node scripts/e2e/foreign-settings.mjs [output-dir]
//
// MEDIAFLICK_DESKTOP_EXE runs another build instead; an installed build from
// before the fix fails the newer-file scenarios, which shows the check bites.
//
// Each scenario starts the staged app (build/mediaflick-desktop.exe) in its
// own throwaway profile under the output directory, reads the app's state
// through its own API over CDP, closes the app, and then compares every
// settings file byte for byte. Failure modes covered:
//
// - a same-version accounts.json with an unknown Home shelf (the incident)
//   is moved aside, replaced by its backup or reset;
// - the same for a same-version accounts.json with an unknown field;
// - a settings.json with an unknown value is moved aside or reset, or the
//   app later saves its defaults over it (a settings change, or on exit);
// - damaged bytes stop recovering from the backup as they did before.
//
// The output directory (default build/e2e/foreign-settings) receives
// report.json with every scenario's API answers and file comparison, plus
// the app log of each run. The script exits non-zero when an invariant fails.

import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises"
import path from "node:path"
import { connect, sleep } from "../website/cdp.mjs"

const root = path.resolve(import.meta.dirname, "../..")
const exe = path.resolve(process.env.MEDIAFLICK_DESKTOP_EXE ?? path.join(root, "build", "mediaflick-desktop.exe"))
const outDir = path.resolve(process.argv[2] ?? path.join(root, "build", "e2e", "foreign-settings"))
const PORT = Number(process.env.CDP_PORT ?? 9361)
if (!existsSync(exe)) throw new Error(`${exe} is missing; run \`just build\` first`)

const json = (value) => `${JSON.stringify(value, null, 2)}\n`
const HOME = { billboard: true, watching: { continueWatching: true, nextUp: true, combine: true } }
const account = (elements, extra = {}) => ({
  version: 1,
  accounts: [{ serverId: "server", userId: "user", appearance: { accent: "violet", rating_sources: ["imdb", "tomatoes"] }, home: { ...HOME, elements }, ...extra }],
})
const KNOWN_HOME = [{ kind: "builtIn", id: "watching", enabled: true }, { kind: "builtIn", id: "recentlyAdded", enabled: true }]
// What a newer build writes: the supported version, with a shelf this one lacks.
const NEWER_ACCOUNTS = json(account([...KNOWN_HOME, { kind: "builtIn", id: "someFutureShelf", enabled: true }]))
const NEWER_FIELD_ACCOUNTS = json({ ...account(KNOWN_HOME), futureOption: { added: true } })
const VALID_ACCOUNTS = json(account(KNOWN_HOME))
const NEWER_SETTINGS = json({ close_behavior: "hibernate", mark_watched_next: "w", log_level: "debug" })
const VALID_SETTINGS = json({ mark_watched_next: "w", log_level: "debug", show_scrollbars: true })

const scenarios = [
  {
    name: "newer-accounts-shelf",
    files: { "accounts.json": NEWER_ACCOUNTS, "accounts.json.bak": VALID_ACCOUNTS, "settings.json": VALID_SETTINGS },
    expect: { untouched: ["accounts.json", "accounts.json.bak", "settings.json"], apiStatus: 503 },
  },
  {
    name: "newer-accounts-field",
    files: { "accounts.json": NEWER_FIELD_ACCOUNTS, "accounts.json.bak": VALID_ACCOUNTS, "settings.json": VALID_SETTINGS },
    expect: { untouched: ["accounts.json", "accounts.json.bak", "settings.json"], apiStatus: 503 },
  },
  {
    name: "newer-settings",
    files: { "settings.json": NEWER_SETTINGS, "settings.json.bak": VALID_SETTINGS, "accounts.json": VALID_ACCOUNTS },
    // The app runs on defaults, and a settings change is refused rather
    // than saved over the newer file.
    patch: { path: "/api/settings/client/application", body: { showScrollbars: true }, status: 400 },
    expect: { untouched: ["settings.json", "settings.json.bak", "accounts.json"], apiStatus: 200 },
  },
  {
    name: "damaged-accounts",
    files: { "accounts.json": `${VALID_ACCOUNTS.slice(0, 40)}`, "accounts.json.bak": VALID_ACCOUNTS, "settings.json": VALID_SETTINGS },
    // Real damage still recovers: the bytes move aside and the backup returns.
    // settings.json is valid here, so the app may save it on exit as usual.
    expect: { untouched: ["accounts.json.bak"], restored: { "accounts.json": VALID_ACCOUNTS }, movedAside: "accounts.json", apiStatus: 200 },
  },
]

async function stopApp() {
  // Only the app started here listens on this debugging port.
  const script = `Get-CimInstance Win32_Process -Filter "Name = 'mediaflick-desktop.exe'" | Where-Object { $_.CommandLine -match '--remote-debugging-port[ =]${PORT}\\b' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`
  await new Promise((resolve) => spawn("pwsh.exe", ["-NoLogo", "-NoProfile", "-Command", script], { stdio: "ignore" }).on("exit", resolve))
  await sleep(1000)
}

// Closes the window the way a user does, so any save-on-exit runs, and waits
// for the process to end.
async function closeApp(pid) {
  const script = `$p = Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if ($p) { [void]$p.CloseMainWindow(); if (-not $p.WaitForExit(15000)) { exit 1 } }`
  const closed = await new Promise((resolve) => spawn("pwsh.exe", ["-NoLogo", "-NoProfile", "-Command", script], { stdio: "ignore" }).on("exit", (code) => resolve(code === 0)))
  await stopApp()
  return closed
}

const report = { exe, checkedAt: new Date().toISOString(), scenarios: [], failures: [] }
await rm(outDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 })

for (const scenario of scenarios) {
  const fail = (message) => report.failures.push(`${scenario.name}: ${message}`)
  const base = path.join(outDir, scenario.name)
  const roaming = path.join(base, "Roaming")
  const local = path.join(base, "Local")
  const config = path.join(roaming, "mediaflick-desktop")
  await mkdir(config, { recursive: true })
  await mkdir(path.join(local, "mediaflick-desktop"), { recursive: true })
  for (const [name, contents] of Object.entries(scenario.files)) await writeFile(path.join(config, name), contents)

  await stopApp()
  const child = spawn(exe, ["--remote-debugging-port", String(PORT), "--log-level", "info"], {
    cwd: path.dirname(exe),
    env: { ...process.env, APPDATA: roaming, LOCALAPPDATA: local },
    detached: true,
    stdio: "ignore",
  })
  child.unref()

  const result = { name: scenario.name }
  try {
    const app = await connect(PORT)
    await app.send("Runtime.enable")
    // The page's own API, as the UI calls it.
    const call = (method, apiPath, body) => app.evaluate(`fetch(${JSON.stringify(apiPath)}, { method: ${JSON.stringify(method)}, headers: { "Content-Type": "application/json" }${body ? `, body: ${JSON.stringify(JSON.stringify(body))}` : ""} }).then(async (response) => ({ status: response.status, text: (await response.text()).slice(0, 600) }))`)
    await sleep(1500)
    result.settings = await call("GET", "/api/settings")
    if (result.settings.status !== scenario.expect.apiStatus) fail(`GET /api/settings answered ${result.settings.status}, expected ${scenario.expect.apiStatus}: ${result.settings.text}`)
    if (scenario.patch) {
      result.patch = await call("PATCH", scenario.patch.path, scenario.patch.body)
      if (result.patch.status !== scenario.patch.status) fail(`PATCH ${scenario.patch.path} answered ${result.patch.status}: ${result.patch.text}`)
    }
    app.close()
  } catch (error) {
    fail(`could not drive the app: ${error?.message ?? error}`)
  }
  result.closedCleanly = await closeApp(child.pid)

  // Every file, byte for byte, after the app has exited.
  const entries = await readdir(config)
  result.files = entries.filter((name) => name.endsWith(".json") || name.includes(".json."))
  for (const name of scenario.expect.untouched) {
    const now = await readFile(path.join(config, name), "utf8").catch(() => null)
    if (now !== scenario.files[name]) fail(`${name} changed: ${now === null ? "missing" : now.slice(0, 200)}`)
  }
  for (const [name, contents] of Object.entries(scenario.expect.restored ?? {})) {
    const now = await readFile(path.join(config, name), "utf8").catch(() => null)
    if (now !== contents) fail(`${name} was not restored from its backup`)
  }
  const movedAside = entries.filter((name) => name.includes(".broken-"))
  if (scenario.expect.movedAside) {
    if (!movedAside.some((name) => name.startsWith(`${scenario.expect.movedAside}.broken-`))) fail(`damaged ${scenario.expect.movedAside} was not kept aside`)
  } else if (movedAside.length) {
    fail(`files were moved aside: ${movedAside.join(", ")}`)
  }
  const log = path.join(config, "mediaflick-desktop.log")
  if (existsSync(log)) {
    await cp(log, path.join(outDir, `${scenario.name}.log`))
    result.log = path.relative(root, path.join(outDir, `${scenario.name}.log`))
  }
  report.scenarios.push(result)
  console.log(`${scenario.name}: ${report.failures.some((failure) => failure.startsWith(`${scenario.name}:`)) ? "failed" : "ok"}`)
}

await writeFile(path.join(outDir, "report.json"), JSON.stringify(report, null, 2))
console.log(`report: ${path.relative(root, path.join(outDir, "report.json"))}`)
if (report.failures.length) {
  console.error(report.failures.map((failure) => `  ✗ ${failure}`).join("\n"))
  process.exitCode = 1
}
