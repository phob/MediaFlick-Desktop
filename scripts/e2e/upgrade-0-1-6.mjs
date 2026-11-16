// End-to-end proof of the update from MediaFlick 0.1.6 to this build, and of
// going back to 0.1.6 (operator decisions of 2026-11-15). Linux only.
//
//   just upgrade-e2e --account https://jellyfin.example.com
//   node scripts/e2e/upgrade-0-1-6.mjs --account SERVER [--only id,id] [--appimage FILE] [--servers servers.json]
//
// Needs the staged build (`just build`), the AppImage of this build
// (`just linux-appimage`, or --appimage) for the packaging scenario, ffmpeg,
// Xvfb, dotnet (the Companion for a local server), and a session cached by
// `just verify-login SERVER` for --account. It downloads the real 0.1.6
// AppImage from the v0.1.6 GitHub release and checks its SHA-256.
//
// Every scenario runs in its own throwaway profile through verify-mediaflick
// (`isolate-linux.sh` + `session.mjs`, one at a time) or, for 0.1.6 itself,
// through scripts/e2e/run-0-1-6.mjs on the same private Xvfb display. The
// 0.1.6 profiles are made by the real 0.1.6 where it can make them; 0.1.6 on
// Linux never offered MPC-HC, so that profile is written by hand the way
// 0.1.6 serialized it.
//
// The failure list each scenario checks is in SCENARIOS below and in the
// drives it names. The artifact is build/e2e/upgrade-0-1-6/report.json, with a
// screenshot and the app log per scenario in its directory. Exits non-zero
// when a scenario fails.
import { execFileSync, spawn } from "node:child_process"
import { createHash } from "node:crypto"
import { copyFileSync, cpSync, createReadStream, createWriteStream, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import path from "node:path"
import { Readable } from "node:stream"
import { pipeline } from "node:stream/promises"
import { parseArgs } from "node:util"

const root = path.resolve(import.meta.dirname, "../..")
const verify = path.join(root, ".claude/skills/verify-mediaflick")
const out = path.join(root, "build/e2e/upgrade-0-1-6")
const DEMO = "https://demo.jellyfin.org/stable"
const MPV = "/usr/bin/mpv"
const LEGACY = {
  release: "v0.1.6",
  name: "MediaFlickDesktop-v0.1.6-linux-x86_64.AppImage",
  // The asset as published on 2026-07-16 (gh release view v0.1.6).
  sha256: "b0892fe0f60a817b8bd8497af64e72537ede7bc2d5bd8e40e6eecac64c1924f3",
}
const VERSIONS = ["10.10.7", "10.11.11", "12.4", "12.4+companion"]

const { values: options } = parseArgs({
  options: {
    account: { type: "string" },
    only: { type: "string" },
    appimage: { type: "string" },
    servers: { type: "string" },
  },
})
if (process.platform !== "linux") throw new Error("the upgrade drive runs on Linux; see `outstanding` in the report for Windows and macOS")
if (!options.account) throw new Error("--account SERVER is required: your own server, cached with `just verify-login SERVER`")
const only = options.only ? new Set(options.only.split(",")) : null
const staged = path.join(root, "build/mediaflick-desktop")
if (!existsSync(staged)) throw new Error("build/mediaflick-desktop is missing; run `just build`")
const appImage = path.resolve(options.appimage ?? (readdirSync(path.join(root, "dist/linux")).filter((name) => name.endsWith(".AppImage")).map((name) => path.join(root, "dist/linux", name))[0] ?? ""))
if (!existsSync(appImage)) throw new Error("no AppImage of this build; run `just linux-appimage` or pass --appimage")

const stamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15)
const sha256 = (file) => new Promise((resolve, reject) => {
  const digest = createHash("sha256")
  createReadStream(file).on("data", (chunk) => digest.update(chunk)).on("end", () => resolve(digest.digest("hex"))).on("error", reject)
})
const git = (...args) => execFileSync("git", ["-C", root, ...args], { encoding: "utf8" }).trim()
const json = (value) => `${JSON.stringify(value, null, 2)}\n`
// 0.1.6 wrote its files with serde_json::to_vec_pretty: two-space indent, no trailing newline.
const pretty = (value) => JSON.stringify(value, null, 2)
const log = (line) => console.log(`[upgrade-0-1-6] ${line}`)

rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })

async function legacyAppImage() {
  const dir = path.join(root, "build/e2e/mediaflick-0.1.6")
  const file = path.join(dir, LEGACY.name)
  if (!existsSync(file) || (await sha256(file)) !== LEGACY.sha256) {
    mkdirSync(dir, { recursive: true })
    const url = `https://github.com/phob/MediaFlick-Desktop/releases/download/${LEGACY.release}/${LEGACY.name}`
    log(`downloading ${url}`)
    const reply = await fetch(url)
    if (!reply.ok) throw new Error(`${url}: HTTP ${reply.status}`)
    await pipeline(Readable.fromWeb(reply.body), createWriteStream(file))
    const digest = await sha256(file)
    if (digest !== LEGACY.sha256) throw new Error(`${LEGACY.name} has SHA-256 ${digest}, not the published ${LEGACY.sha256}`)
  }
  execFileSync("chmod", ["+x", file])
  return file
}

// The local Jellyfin servers: below the 10.11.0 floor, a supported 10.11, and
// Jellyfin 12 without and with the Companion, each with the administrator
// e2e-admin. Started here unless --servers names ones already running.
async function localServers() {
  if (options.servers) return { servers: JSON.parse(readFileSync(options.servers, "utf8")), stop: () => {} }
  const script = path.join(verify, "scripts/jellyfin-version-servers.py")
  const child = spawn("python3", [script, ...VERSIONS], { cwd: root, stdio: ["ignore", "pipe", "pipe"] })
  const sink = createWriteStream(path.join(out, "jellyfin-servers.log"))
  child.stderr.pipe(sink)
  await new Promise((resolve, reject) => {
    let seen = ""
    child.stdout.on("data", (chunk) => {
      sink.write(chunk)
      seen += chunk
      if (/^ready$/m.test(seen)) resolve()
    })
    child.once("exit", (code) => reject(new Error(`jellyfin-version-servers.py exited with ${code}; see ${path.join(out, "jellyfin-servers.log")}`)))
  })
  const file = path.join(root, "build/e2e/jellyfin-version-servers/servers.json")
  return { servers: JSON.parse(readFileSync(file, "utf8")), stop: () => child.kill("SIGTERM") }
}

function wrapped(command, env = {}, timeoutS = 900) {
  return new Promise((resolve) => {
    const child = spawn("bash", [path.join(verify, "scripts/isolate-linux.sh"), ...command], {
      cwd: root,
      env: { ...process.env, MEDIAFLICK_VERIFY_HARD_TIMEOUT: String(timeoutS), ...env },
      stdio: ["ignore", "pipe", "pipe"],
    })
    const chunks = []
    child.stdout.on("data", (chunk) => chunks.push(chunk))
    child.stderr.on("data", (chunk) => chunks.push(chunk))
    // Daemons the wrapper's session bus started can hold the pipes open after it exits.
    child.once("exit", (code) => {
      child.stdout.destroy()
      child.stderr.destroy()
      resolve({ code, output: Buffer.concat(chunks).toString("utf8") })
    })
  })
}

// One verify-mediaflick session; its evidence is build/verify/<run id>.
async function session(id, drive, args, env, timeoutS) {
  const runId = `upgrade-${id}-${stamp}`
  log(`${id}: just verify ${drive} ${args.join(" ")}`)
  const { code, output } = await wrapped(["node", path.join(verify, "scripts/session.mjs"), drive, "--run-id", runId, ...args], env, timeoutS)
  const evidence = path.join(root, "build/verify", runId)
  writeFileSync(path.join(evidence, "wrapper.log"), output)
  return { runId, code, evidence }
}

// The real 0.1.6 once on a profile; its record is <dir>/run.json.
async function legacyRun(legacy, profile, dir, args) {
  log(`0.1.6 on ${path.relative(root, profile)} ${args.join(" ")}`)
  const { code, output } = await wrapped(["node", path.join(root, "scripts/e2e/run-0-1-6.mjs"), "--appimage", legacy, "--profile", profile, "--out", dir, ...args], {}, 300)
  mkdirSync(dir, { recursive: true })
  writeFileSync(path.join(dir, "wrapper.log"), output)
  const runFile = path.join(dir, "run.json")
  if (code !== 0 || !existsSync(runFile)) throw new Error(`run-0-1-6.mjs failed (${code}); see ${path.join(dir, "wrapper.log")}`)
  return JSON.parse(readFileSync(runFile, "utf8"))
}

// A session's result for the report: its checks, and its screenshots and log
// copied into the scenario's directory.
function collect(id, run, { checksFile, screenshot }) {
  const dir = path.join(out, id)
  mkdirSync(dir, { recursive: true })
  const result = existsSync(path.join(run.evidence, "result.json")) ? JSON.parse(readFileSync(path.join(run.evidence, "result.json"), "utf8")) : null
  for (const name of readdirSync(run.evidence)) {
    if (/\.(png|json|log|txt)$/.test(name)) copyFileSync(path.join(run.evidence, name), path.join(dir, name))
  }
  const checks = checksFile && existsSync(path.join(run.evidence, checksFile)) ? JSON.parse(readFileSync(path.join(run.evidence, checksFile), "utf8")) : null
  const shots = readdirSync(dir).filter((name) => name.endsWith(".png")).sort()
  return {
    runId: run.runId,
    evidence: path.relative(root, run.evidence),
    passed: run.code === 0 && result?.passed === true,
    error: result?.error?.split("\n")[0] ?? (run.code === 0 ? null : `session exited ${run.code}`),
    checks: checks?.checks ?? null,
    report: checks,
    screenshot: path.relative(root, path.join(dir, shots.includes(screenshot) ? screenshot : shots.at(-1) ?? "")),
    screenshots: shots,
    log: existsSync(path.join(dir, "app.log")) ? path.relative(root, path.join(dir, "app.log")) : null,
  }
}

// The verify layout run-0-1-6.mjs and session.mjs share: config/ and data/ roots.
const appDirs = (profile) => ({ config: path.join(profile, "config/mediaflick-desktop"), data: path.join(profile, "data/mediaflick-desktop") })

const report = {
  generatedAt: new Date().toISOString(),
  git: { head: git("rev-parse", "HEAD"), branch: git("rev-parse", "--abbrev-ref", "HEAD"), dirty: git("status", "--porcelain") !== "" },
  builds: { staged: { path: path.relative(root, staged), modified: statSync(staged).mtime.toISOString() }, appImage: { path: path.relative(root, appImage), sha256: await sha256(appImage) } },
  account: options.account,
  scenarios: [],
  outstanding: OUTSTANDING(),
  failures: [],
}
const legacy = await legacyAppImage()
report.builds.legacy = { release: LEGACY.release, path: path.relative(root, legacy), sha256: LEGACY.sha256 }
const { servers, stop } = await localServers()
report.servers = servers.map(({ version, requested, url, companion }) => ({ version, requested, url, companion }))
const server = (requested) => {
  const found = servers.find((entry) => (entry.requested ?? entry.version) === requested)
  if (!found) throw new Error(`no local Jellyfin ${requested} in the servers file`)
  return found
}

// Profiles the real 0.1.6 made: on the demo server signed in to Jellyfin Web
// (a 0.1.6 user's session lives in the cef-cache), and on your own server.
// Both saved an mpv path, as 0.1.6's --mpv-path and its mpv setup did.
const profiles = path.join(out, "0.1.6-profiles")
const made = {}
async function legacyProfile(name, args) {
  if (made[name]) return made[name]
  const profile = path.join(profiles, name, "profile")
  const run = await legacyRun(legacy, profile, path.join(profiles, name, "0.1.6"), args)
  if (!run.page || run.exit?.code !== 0) throw new Error(`0.1.6 did not make the ${name} profile: ${JSON.stringify({ page: run.page, exit: run.exit, problems: run.problems })}`)
  made[name] = { profile, ...appDirs(profile), run }
  return made[name]
}

const SCENARIOS = [
  {
    id: "2-external-mpv",
    title: "A 0.1.6 profile with an external mpv path still plays through external mpv",
    async run() {
      const made = await legacyProfile("own-server", ["--url", options.account, "--mpv-path", MPV])
      // Windowed, as a user sets it in 0.1.6's settings dialog: fullscreen
      // external mpv covers the app and freezes CDP mouse input under Xvfb.
      const config = path.join(out, this.id, "seed")
      cpSync(made.config, config, { recursive: true })
      const legacyConfig = JSON.parse(readFileSync(path.join(config, "config.json"), "utf8"))
      writeFileSync(path.join(config, "config.json"), pretty({ ...legacyConfig, default_fullscreen: "windowed" }))
      const run = await session(this.id, "upgrade-external-mpv", ["--account", options.account, "--seed", config, "--seed-data", made.data, "--timeout", "900", "--log-level", "debug,jellyfin.playstate=trace,playback=trace"], {}, 1200)
      return { ...collect(this.id, run, { checksFile: "upgrade-external-mpv.json", screenshot: "99-after-playback.png" }), notes: ["Profile made by the real 0.1.6 (--url, --mpv-path); default_fullscreen set to windowed as 0.1.6's settings dialog writes it."] }
    },
  },
  {
    id: "3-mpc-hc",
    title: "A 0.1.6 profile that selected MPC-HC falls back to the Built-in player and keeps its other settings",
    async run() {
      const run = await session(this.id, "upgrade-0-1-6", ["--seed", path.join(verify, "seeds/upgrade-0-1-6")], {}, 600)
      return { ...collect(this.id, run, { checksFile: "upgrade-0-1-6.json", screenshot: "02-player-imported.png" }), notes: ["0.1.6 offered MPC-HC on Windows only; the seed is config.json and input.json as 0.1.6 serialized them."] }
    },
  },
  {
    id: "4-config-only",
    title: "A profile with only config.json and input.json: the import carries every setting over",
    async run() {
      // Every field 0.1.6 could save, off its default, in AppSettings' field
      // order (v0.1.6:src/preferences/model.rs), and a disabled mark-watched key.
      const seed = path.join(out, this.id, "seed")
      mkdirSync(seed, { recursive: true })
      writeFileSync(path.join(seed, "config.json"), pretty({
        jellyfin_url: DEMO,
        mpv_path: MPV,
        log_level: "info",
        default_fullscreen: "windowed",
        streaming_quality: "auto",
        show_scrollbars: true,
        webui_window: { width: 1600, height: 900, maximized: true },
        skip_intro: "always",
        skip_credits: "disabled",
        skip_recap: "disabled",
        skip_commercial: "always",
      }))
      writeFileSync(path.join(seed, "input.json"), pretty({ bindings: { mark_watched_next: "" } }))
      const run = await session(this.id, "upgrade-0-1-6", ["--seed", seed], {}, 600)
      return { ...collect(this.id, run, { checksFile: "upgrade-0-1-6.json", screenshot: "02-player-imported.png" }), notes: ["close_behavior stays at its default: the drive's relaunch closes the window."] }
    },
  },
  {
    id: "5-appimage",
    title: "The AppImage starts on a 0.1.6 profile directory",
    async run() {
      const made = await legacyProfile("demo", ["--url", DEMO, "--mpv-path", MPV, "--web-user", "demo"])
      const run = await session(this.id, "upgrade-0-1-6", ["--seed", made.config, "--seed-data", made.data], { MEDIAFLICK_DESKTOP_EXE: appImage }, 600)
      return { ...collect(this.id, run, { checksFile: "upgrade-0-1-6.json", screenshot: "02-player-imported.png" }), notes: [`${path.relative(root, appImage)} on the config and data dirs (CEF ${made.run.files.cefCache.after.lastChromeVersion} cache) the real 0.1.6 AppImage left.`] }
    },
  },
  {
    id: "6-old-server",
    title: "A 0.1.6 profile on a server below 10.11.0 gets the explanation on the sign-in screen",
    async run() {
      const old = server("10.10.7")
      const dir = path.join(out, this.id)
      const seed = path.join(dir, "seed")
      mkdirSync(seed, { recursive: true })
      writeFileSync(path.join(seed, "config.json"), pretty({ jellyfin_url: old.url, mpv_path: MPV }))
      // Only the old server, then the drive's read-only demo pass.
      const serversFile = path.join(dir, "servers.json")
      writeFileSync(serversFile, json([old]))
      const run = await session(this.id, "server-versions", ["--seed", seed, "--timeout", "600"], { MEDIAFLICK_VERIFY_VERSION_SERVERS: serversFile }, 900)
      return collect(this.id, run, { checksFile: "server-versions.json", screenshot: `${old.version}-refused-on-start.png` })
    },
  },
  {
    id: "whats-new-member-companion",
    title: "What's new on a 0.1.6 profile, your own server with the Companion, non-administrator: shown once, no install offer, not again after a restart",
    async run() {
      const made = await legacyProfile("own-server", ["--url", options.account, "--mpv-path", MPV])
      const run = await session(this.id, "whats-new", ["--account", options.account, "--signed-out", "--seed", made.config, "--seed-data", made.data], {}, 600)
      return collect(this.id, run, { checksFile: "whats-new.json", screenshot: "01-whats-new.png" })
    },
  },
  {
    id: "whats-new-fresh",
    title: "No What's new for a fresh profile",
    async run() {
      const run = await session(this.id, "whats-new", ["--account", options.account, "--signed-out"], {}, 600)
      return collect(this.id, run, { checksFile: "whats-new.json", screenshot: "01-companion-settings.png" })
    },
  },
  ...[
    ["whats-new-admin-no-companion", "12.4", "an administrator on Jellyfin 12 without the Companion gets the install offer"],
    ["whats-new-admin-companion", "12.4+companion", "an administrator on Jellyfin 12 with the Companion installed gets no install offer"],
    ["whats-new-admin-10-11", "10.11.11", "an administrator on Jellyfin 10.11 is not offered a Companion that needs 12.0.0"],
  ].map(([id, requested, title]) => ({
    id,
    title: `What's new on a 0.1.6 profile: ${title}`,
    async run() {
      const local = server(requested)
      const seed = path.join(out, id, "seed")
      mkdirSync(seed, { recursive: true })
      writeFileSync(path.join(seed, "config.json"), pretty({ jellyfin_url: local.url, mpv_path: MPV }))
      const env = { MEDIAFLICK_VERIFY_WHATS_NEW_SERVER: local.url, MEDIAFLICK_VERIFY_WHATS_NEW_USER: local.username, MEDIAFLICK_VERIFY_WHATS_NEW_PASSWORD: local.password }
      const run = await session(id, "whats-new", ["--seed", seed], env, 600)
      return collect(id, run, { checksFile: "whats-new.json", screenshot: "01b-whats-new-end.png" })
    },
  })),
  {
    id: "8-way-back",
    title: "The way back: the real 0.1.6 on a profile 0.2.0 has used, then this build again",
    async run() {
      const dir = path.join(out, this.id)
      const made = await legacyProfile("demo", ["--url", DEMO, "--mpv-path", MPV, "--web-user", "demo"])
      // 1. This build on the 0.1.6 profile: sign in, close What's new, relaunch; keep the profile.
      const used = await session(`${this.id}-0.2.0`, "whats-new", ["--seed", made.config, "--seed-data", made.data, "--keep-profile"], {}, 600)
      const first = collect(`${this.id}/1-this-build`, used, { checksFile: "whats-new.json", screenshot: "03-after-relaunch.png" })
      const kept = JSON.parse(readFileSync(path.join(used.evidence, "pids.json"), "utf8")).profile
      const profile = path.join(dir, "profile")
      for (const kind of ["config", "data"]) cpSync(path.join(kept, kind), path.join(profile, kind), { recursive: true })
      rmSync(kept, { recursive: true, force: true })
      const beforeLegacy = Object.fromEntries(readdirSync(appDirs(profile).config).filter((name) => name.endsWith(".json")).map((name) => [name, readFileSync(path.join(appDirs(profile).config, name), "utf8")]))
      // 2. The real 0.1.6 on it, as a user who installs 0.1.6 again.
      const back = await legacyRun(legacy, profile, path.join(dir, "2-0.1.6"), [])
      // 3. This build again on what 0.1.6 left.
      const again = await session(`${this.id}-return`, "upgrade-return", ["--seed", appDirs(profile).config, "--seed-data", appDirs(profile).data], {}, 600)
      const third = collect(`${this.id}/3-this-build-again`, again, { checksFile: "upgrade-return.json", screenshot: "01-home.png" })
      const legacyChecks = [
        { ok: back.page !== null, message: "0.1.6 starts and shows a page", detail: back.page?.href ?? back.problems },
        { ok: back.exit?.code === 0 && back.exit.how === "closed its window", message: "0.1.6 closes cleanly", detail: back.exit },
        { ok: back.leftovers.length === 0, message: "no 0.1.6 process outlives it", detail: back.leftovers },
        ...[["config", "settings.json"], ["config", "accounts.json"], ["data", "library.db"]].map(([kind, name]) => ({
          ok: !back.files[kind].changed.includes(name) && !back.files[kind].removed.includes(name),
          message: `0.1.6 leaves this build's ${name} as it was`,
        })),
      ]
      const passed = first.passed && third.passed && legacyChecks.every((check) => check.ok)
      return {
        passed,
        error: passed ? null : [first.error, third.error, ...legacyChecks.filter((check) => !check.ok).map((check) => check.message)].filter(Boolean).join("; "),
        checks: [...(first.checks ?? []), ...legacyChecks, ...(third.checks ?? [])],
        screenshot: path.relative(root, path.join(dir, "2-0.1.6", back.screenshot ?? back.display)),
        screenshots: [first.screenshot, path.relative(root, path.join(dir, "2-0.1.6", back.screenshot ?? back.display)), path.relative(root, path.join(dir, "2-0.1.6", back.display)), third.screenshot],
        log: path.relative(root, path.join(dir, "2-0.1.6", "app.log")),
        logs: [first.log, path.relative(root, path.join(dir, "2-0.1.6", "app.log")), third.log],
        steps: { thisBuild: first.runId, legacy: path.relative(root, path.join(dir, "2-0.1.6", "run.json")), thisBuildAgain: third.runId },
        wayBack: {
          page: back.page && { href: back.page.href, title: back.page.title, signedInToJellyfinWeb: back.page.href.includes("#/home") },
          exit: back.exit,
          config: back.files.config,
          data: back.files.data,
          cefCache: back.files.cefCache,
          configJsonBefore: beforeLegacy["config.json"] ?? null,
          configJsonAfter: back.textAfter["config.json"] ?? null,
          settingsJsonUntouched: back.textAfter["settings.json"] === beforeLegacy["settings.json"],
          returnedSignedIn: third.report?.status?.authenticated ?? null,
        },
      }
    },
  },
]

try {
  for (const scenario of SCENARIOS) {
    if (only && !only.has(scenario.id)) continue
    let result
    try {
      result = await scenario.run()
    } catch (error) {
      result = { passed: false, error: error.message }
    }
    report.scenarios.push({ id: scenario.id, title: scenario.title, ...result })
    if (!result.passed) report.failures.push(`${scenario.id}: ${result.error ?? "failed"}`)
    log(`${scenario.id}: ${result.passed ? "passed" : `FAILED ${result.error ?? ""}`}`)
    writeFileSync(path.join(out, "report.json"), json(report))
  }
} finally {
  stop()
  report.legacyProfiles = Object.fromEntries(Object.entries(made).map(([name, entry]) => [name, { profile: path.relative(root, entry.profile), page: entry.run.page?.href ?? null, configJson: entry.run.textAfter["config.json"] ?? null, cefCache: entry.run.files.cefCache.after }]))
  writeFileSync(path.join(out, "report.json"), json(report))
}
log(`report: ${path.relative(root, path.join(out, "report.json"))}`)
for (const scenario of report.scenarios) log(`  ${scenario.passed ? "ok  " : "FAIL"} ${scenario.id}`)
if (report.failures.length) process.exitCode = 1

// What Linux cannot run, for someone on Windows or a Mac, step by step.
function OUTSTANDING() {
  return [
    {
      id: "1-windows-installer-update",
      platform: "Windows",
      title: "Windows: the 0.1.6 installer, then this build's installer as 0.1.6's updater runs it",
      steps: [
        "On a Windows 11 machine or VM without MediaFlick, download MediaFlickDesktop-Setup-0.1.6.exe from the v0.1.6 release and install it.",
        "Start it, enter your Jellyfin server, sign in to Jellyfin Web, and in its settings choose External mpv with an mpv path (or MPC-HC on a second run).",
        "Close it. Note %APPDATA%\\mediaflick-desktop\\config.json and the install directory (%LOCALAPPDATA%\\Programs\\MediaFlick Desktop by default).",
        "Build or download this build's MediaFlickDesktop-Setup-<version>.exe and run it with the arguments 0.1.6's updater uses (`git show v0.1.6:src/maintenance/updater.rs`, start_installer).",
        "Expected: the installer upgrades in place, the app restarts on the sign-in screen with the 0.1.6 server filled in, Settings → Player shows External mpv with the same path (Built-in player after MPC-HC), and after sign-in What's new opens once.",
        "Record a screenshot of the sign-in screen and %APPDATA%\\mediaflick-desktop\\mediaflick-desktop.log.",
      ],
    },
    {
      id: "7-windows-leftover-files",
      platform: "Windows",
      title: "Windows: files 0.1.6 left in the install directory do not break startup",
      steps: [
        "After scenario 1, list the install directory and note files this build's installer did not ship (0.1.6's MPC-HC or mpv helpers, old CEF files).",
        "Start MediaFlick from the Start menu: it must reach the sign-in screen or Home without an error dialog, and play a title with the Built-in player.",
        "Also unzip the 0.1.6 portable zip, then this build's zip over it, and start mediaflick-desktop.exe from there: same expectation.",
        "Record a screenshot and the app log for both.",
      ],
    },
    {
      id: "5b-macos-dmg",
      platform: "macOS",
      title: "macOS: the DMG starts on a 0.1.6 profile",
      steps: [
        "On a Mac, install MediaFlickDesktop-v0.1.6-macos-<arch>.dmg from the v0.1.6 release, start it, enter your server, close it.",
        "Install this build's DMG over it (drag to Applications, replace), open it past Gatekeeper (right-click → Open, the app is ad-hoc signed).",
        "Expected: the sign-in screen with the 0.1.6 server filled in; ~/Library/Application Support/mediaflick-desktop/config.json unchanged; Settings → Player shows the 0.1.6 player.",
        "Then install the 0.1.6 DMG again over it and start it: record what it shows (compare with 8-way-back in this report).",
        "Record a screenshot and ~/Library/Application Support/mediaflick-desktop/mediaflick-desktop.log for each start.",
      ],
    },
  ]
}
