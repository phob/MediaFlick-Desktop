// Runs the real MediaFlick 0.1.6 Linux AppImage once on a profile directory
// and records what it did there. Run it inside the verify-mediaflick Xvfb
// wrapper, which keeps it off the user's screen; scripts/e2e/upgrade-0-1-6.mjs
// does that:
//
//   bash .claude/skills/verify-mediaflick/scripts/isolate-linux.sh node scripts/e2e/run-0-1-6.mjs \
//     --appimage <0.1.6 AppImage> --profile <dir> --out <dir> [--url SERVER] [--mpv-path PATH] [--web-user NAME [--web-password PASSWORD]]
//
// The profile directory has the verify layout: config/ and data/ are the
// XDG config and data roots, so the app's own directories are
// config/mediaflick-desktop and data/mediaflick-desktop. A missing profile
// starts empty, which is how a 0.1.6 profile is made for the upgrade checks.
//
// 0.1.6 shows Jellyfin Web in CEF. The script waits for its page over CDP,
// with --web-user signs in to Jellyfin Web as a 0.1.6 user did (the session
// then lives in the cef-cache), captures the page and the whole display, closes the window the way the page's
// own close does (0.1.6 saves config.json then), and writes run.json to the
// output directory: the page it showed, how it exited, and every file it
// added, changed or removed in the config dir and the data dir (cef-cache
// summarized), with the part of mediaflick-desktop.log it wrote as app.log.
import { execFileSync, spawn } from "node:child_process"
import { createHash } from "node:crypto"
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs"
import net from "node:net"
import path from "node:path"
import { parseArgs } from "node:util"
import { assertIsolated, killPids, listProcesses } from "../../.claude/skills/verify-mediaflick/scripts/platform.mjs"
import { connect, sleep } from "../website/cdp.mjs"

const { values: options } = parseArgs({
  options: {
    appimage: { type: "string" },
    profile: { type: "string" },
    out: { type: "string" },
    url: { type: "string" },
    "mpv-path": { type: "string" },
    "web-user": { type: "string" },
    "web-password": { type: "string", default: "" },
    settle: { type: "string", default: "8" },
  },
})
for (const name of ["appimage", "profile", "out"]) if (!options[name]) throw new Error(`--${name} is required`)
const isolation = assertIsolated()
const appImage = path.resolve(options.appimage)
const profile = path.resolve(options.profile)
const out = path.resolve(options.out)
mkdirSync(out, { recursive: true })
const configDir = path.join(profile, "config", "mediaflick-desktop")
const dataDir = path.join(profile, "data", "mediaflick-desktop")
for (const dir of [configDir, dataDir, path.join(profile, "home"), path.join(profile, "cache"), path.join(profile, "state")]) mkdirSync(dir, { recursive: true })

const hash = (file) => createHash("sha256").update(readFileSync(file)).digest("hex")
function walk(dir, prefix = "") {
  const entries = {}
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name)
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) Object.assign(entries, walk(full, relative))
    else if (entry.isFile()) entries[relative] = { size: statSync(full).size, sha256: hash(full) }
  }
  return entries
}
// The cef-cache holds hundreds of Chromium files; its version stamp and size say what matters.
function manifest() {
  const data = walk(dataDir)
  const cache = Object.entries(data).filter(([name]) => name.startsWith("cef-cache/"))
  // Chromium stamps the profile with the last browser version that opened it.
  const preferences = path.join(dataDir, "cef-cache", "Default", "Preferences")
  const stamp = existsSync(preferences) ? JSON.parse(readFileSync(preferences, "utf8")) : null
  return {
    config: walk(configDir),
    data: Object.fromEntries(Object.entries(data).filter(([name]) => !name.startsWith("cef-cache/"))),
    cefCache: {
      files: cache.length,
      bytes: cache.reduce((sum, [, entry]) => sum + entry.size, 0),
      lastChromeVersion: stamp?.extensions?.last_chrome_version ?? null,
      createdByVersion: stamp?.profile?.created_by_version ?? null,
      digest: createHash("sha256").update(JSON.stringify(cache)).digest("hex"),
    },
  }
}
function diff(before, after) {
  const names = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort()
  return {
    added: names.filter((name) => !before[name] && after[name]),
    removed: names.filter((name) => before[name] && !after[name]),
    changed: names.filter((name) => before[name] && after[name] && before[name].sha256 !== after[name].sha256),
  }
}

const freePort = () => new Promise((resolve, reject) => {
  const server = net.createServer()
  server.once("error", reject)
  server.listen(0, "127.0.0.1", () => {
    const { port } = server.address()
    server.close(() => resolve(port))
  })
})

const logFile = path.join(configDir, "mediaflick-desktop.log")
const logBefore = existsSync(logFile) ? readFileSync(logFile) : Buffer.alloc(0)
const before = manifest()
const textFiles = {}
for (const name of ["config.json", "input.json", "settings.json", "instance.json"]) {
  const file = path.join(configDir, name)
  if (existsSync(file)) textFiles[name] = readFileSync(file, "utf8")
}

const env = {
  ...process.env,
  HOME: path.join(profile, "home"),
  XDG_CONFIG_HOME: path.join(profile, "config"),
  XDG_DATA_HOME: path.join(profile, "data"),
  XDG_CACHE_HOME: path.join(profile, "cache"),
  XDG_STATE_HOME: path.join(profile, "state"),
  // Chromium's singleton socket must fit the 107-byte Unix path limit.
  TMPDIR: process.env.XDG_RUNTIME_DIR,
}
for (const name of ["LD_PRELOAD", "LD_LIBRARY_PATH", "JELLYFIN_URL", "MEDIAFLICK_DESKTOP_MPV_PATH", "MEDIAFLICK_DESKTOP_LOG_FILE", "MEDIAFLICK_DESKTOP_LOG_LEVEL", "MEDIAFLICK_DESKTOP_REMOTE_DEBUGGING_PORT"]) delete env[name]

const port = await freePort()
const args = ["--remote-debugging-port", String(port), "--log-level", "debug"]
if (options.url) args.push("--url", options.url)
if (options["mpv-path"]) args.push("--mpv-path", options["mpv-path"])
const startedAt = new Date().toISOString()
const child = spawn(appImage, args, { cwd: profile, env, stdio: ["ignore", "pipe", "pipe"] })
const output = []
child.stdout.on("data", (chunk) => output.push(chunk))
child.stderr.on("data", (chunk) => output.push(chunk))
let exit = null
const exited = new Promise((resolve) => child.once("exit", (code, signal) => resolve((exit = { code, signal }))))

const run = { appImage, appImageSha256: hash(appImage), isolation, profile, args, startedAt, page: null, problems: [] }
const capture = (name) => {
  const file = path.join(out, name)
  execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-f", "x11grab", "-i", process.env.DISPLAY, "-frames:v", "1", "-y", file], { stdio: "ignore", timeout: 20000 })
  return path.basename(file)
}

try {
  const page = await Promise.race([
    connect(port, (target) => !target.url.startsWith("devtools://")),
    exited.then(({ code, signal }) => {
      throw new Error(`0.1.6 exited before showing a page (code ${code}, signal ${signal})`)
    }),
  ])
  await page.send("Page.enable")
  await page.waitFor("document.readyState === 'complete'", { timeout: 60000 }).catch(() => run.problems.push("the page never finished loading"))
  await sleep(Number(options.settle) * 1000)
  if (options["web-user"]) {
    // Jellyfin Web's own manual sign-in form; the user cards hide it behind "Manual Login".
    const visible = (selector) => `(() => { const e = document.querySelector(${JSON.stringify(selector)}); return !!e && e.offsetParent !== null })()`
    await page.evaluate(`document.querySelector(".btnManual")?.click()`)
    await page.waitFor(visible("#txtManualName"), { timeout: 30000 })
    await page.evaluate(`(() => {
      document.querySelector("#txtManualName").value = ${JSON.stringify(options["web-user"])}
      document.querySelector("#txtManualPassword").value = ${JSON.stringify(options["web-password"])}
      document.querySelector(".manualLoginForm button[type=submit]").click()
    })()`)
    await page.waitFor(`location.hash.startsWith("#/home")`, { timeout: 30000 })
    await sleep(Number(options.settle) * 1000)
    run.webSignIn = options["web-user"]
  }
  run.page = await page.evaluate(`({ href: location.href, title: document.title, text: (document.body?.innerText ?? "").slice(0, 2000) })`)
  run.targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json().then((targets) => targets.map(({ type, url, title }) => ({ type, url, title })))
  const shot = await page.send("Page.captureScreenshot", { format: "png" })
  writeFileSync(path.join(out, "page.png"), Buffer.from(shot.data, "base64"))
  run.screenshot = "page.png"
  run.display = capture("display.png")
  // The page's own close is the window's close: CEF closes the browser and 0.1.6 saves config.json on the way out.
  // The page goes away with the window, so the call may never be answered.
  await Promise.race([page.evaluate("window.close()").catch(() => {}), sleep(2000)])
  page.close()
} catch (error) {
  run.problems.push(error.message)
  if (exit === null) run.display = capture("display.png")
}

const closed = await Promise.race([exited, sleep(20000).then(() => null)])
run.exit = closed ? { ...closed, how: "closed its window" } : null
if (!closed) {
  child.kill("SIGTERM")
  const terminated = await Promise.race([exited, sleep(10000).then(() => null)])
  run.exit = terminated ? { ...terminated, how: "SIGTERM after the window close did not end it within 20 s" } : null
  if (!terminated) {
    killPids([child.pid], { tree: true })
    run.exit = { ...(await exited), how: "SIGKILL" }
  }
}
// CEF helpers carry --user-data-dir inside the profile; none may outlive the app.
await sleep(1000)
const leftovers = listProcesses().filter((row) => row.cmd.includes(`--user-data-dir=${profile}`))
run.leftovers = leftovers.map(({ pid, name }) => ({ pid, name }))
killPids(leftovers.map((row) => row.pid))

const after = manifest()
run.files = {
  config: diff(before.config, after.config),
  data: diff(before.data, after.data),
  cefCache: { before: before.cefCache, after: after.cefCache, changed: before.cefCache.digest !== after.cefCache.digest },
}
run.textBefore = textFiles
run.textAfter = {}
for (const name of new Set([...Object.keys(textFiles), "config.json", "input.json", "settings.json", "instance.json"])) {
  const file = path.join(configDir, name)
  if (existsSync(file)) run.textAfter[name] = readFileSync(file, "utf8")
}
const logAfter = existsSync(logFile) ? readFileSync(logFile) : Buffer.alloc(0)
const appended = logAfter.subarray(0, logBefore.length).equals(logBefore) ? logAfter.subarray(logBefore.length) : logAfter
run.log = { file: "app.log", truncatedEarlierLog: appended === logAfter && logBefore.length > 0 }
writeFileSync(path.join(out, "app.log"), appended)
writeFileSync(path.join(out, "stdout.log"), Buffer.concat(output))
const cefLog = path.join(dataDir, "cef.log")
if (existsSync(cefLog)) writeFileSync(path.join(out, "cef.log"), readFileSync(cefLog))
writeFileSync(path.join(out, "run.json"), `${JSON.stringify(run, null, 2)}\n`)
console.log(`0.1.6 ${run.page ? `showed ${run.page.href}` : "showed no page"}; exit ${JSON.stringify(run.exit)}; ${path.join(out, "run.json")}`)
