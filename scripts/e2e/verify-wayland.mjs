import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import net from "node:net"
import os from "node:os"
import path from "node:path"
import { parseArgs } from "node:util"
import { connect, sleep } from "../website/cdp.mjs"

const root = path.resolve(import.meta.dirname, "../..")
const buildDir = path.join(root, "build")
const exe = path.resolve(process.env.MEDIAFLICK_DESKTOP_EXE ?? path.join(buildDir, "mediaflick-desktop"))
const outDir = path.join(buildDir, "e2e", "wayland")
const { values: options } = parseArgs({ options: { account: { type: "string" } } })

if (process.platform !== "linux") throw new Error("verify-wayland.mjs supports Linux only")
if (!process.env.WAYLAND_DISPLAY) throw new Error("WAYLAND_DISPLAY is not set; run against a live Wayland compositor")
if (!existsSync(exe)) throw new Error(`${exe} is missing; run \`just build\` first`)

function allocatePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer()
    server.once("error", reject)
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address()
      server.close(() => resolve(port))
    })
  })
}

async function waitForLog(logFile, pattern, label, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const text = await readFile(logFile, "utf8").catch(() => "")
    if (pattern.test(text)) return text
    await sleep(200)
  }
  const latest = await readFile(logFile, "utf8").catch(() => "")
  throw new Error(`timed out waiting for log pattern (${label}):\n${latest.slice(-2000)}`)
}

function buildIsolatedEnv(profileRoot, logFile) {
  const env = { ...process.env }
  delete env.MEDIAFLICK_FORCE_X11
  delete env.MEDIAFLICK_DESKTOP_REMOTE_DEBUGGING_PORT
  delete env.MEDIAFLICK_DESKTOP_LOG_LEVEL
  env.HOME = path.join(profileRoot, "home")
  env.XDG_CONFIG_HOME = path.join(profileRoot, "config")
  env.XDG_DATA_HOME = path.join(profileRoot, "data")
  env.XDG_CACHE_HOME = path.join(profileRoot, "cache")
  env.XDG_STATE_HOME = path.join(profileRoot, "state")
  env.MEDIAFLICK_DESKTOP_LOG_FILE = logFile
  const cef = path.join(buildDir, "libcef.so")
  if (exe === path.join(buildDir, "mediaflick-desktop")) {
    env.LD_LIBRARY_PATH = [buildDir, process.env.LD_LIBRARY_PATH].filter(Boolean).join(":")
    if (existsSync(cef)) {
      env.MEDIAFLICK_DESKTOP_CEF_PRELOAD = cef
      env.LD_PRELOAD = [cef, process.env.LD_PRELOAD].filter(Boolean).join(" ")
    }
  }
  return env
}

async function stopChild(child, exited, app) {
  const signalGroup = (sig) => {
    try {
      process.kill(-child.pid, sig)
    } catch {
      try {
        child.kill(sig)
      } catch {
      }
    }
  }
  if (child.exitCode !== null || child.signalCode !== null) {
    signalGroup("SIGKILL")
    child.stderr?.destroy()
    return { code: child.exitCode, signal: child.signalCode }
  }
  try {
    await app?.send("Browser.close")
  } catch {
  }
  app?.close()
  const winner = await Promise.race([exited, sleep(5000).then(() => null)])
  if (winner) {
    signalGroup("SIGKILL")
    child.stderr?.destroy()
    return winner
  }
  signalGroup("SIGTERM")
  const termed = await Promise.race([exited, sleep(4000).then(() => null)])
  if (termed) {
    signalGroup("SIGKILL")
    child.stderr?.destroy()
    return termed
  }
  signalGroup("SIGKILL")
  child.stderr?.destroy()
  return exited
}

async function runPhase({ name, bwrapDri, requiredLogs }) {
  const profileRoot = await mkdtemp(path.join(os.tmpdir(), `mf-wayland-${name}-`))
  const logFile = path.join(profileRoot, "app.log")
  const screenshotPath = path.join(outDir, `${name}.png`)
  const port = await allocatePort()
  const env = buildIsolatedEnv(profileRoot, logFile)
  const appArgs = ["--remote-debugging-port", String(port), "--log-level", "debug,cef.osr=debug,wayland=debug"]
  const [cmd, args] = bwrapDri
    ? ["bwrap", ["--die-with-parent", "--dev-bind", "/", "/", "--tmpfs", "/dev/dri", "--", exe, ...appArgs]]
    : [exe, appArgs]

  const child = spawn(cmd, args, { env, detached: true, stdio: ["ignore", "ignore", "pipe"] })
  const exited = new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })))
  let stderr = ""
  child.stderr?.on("data", (chunk) => {
    stderr += chunk.toString("utf8")
  })
  let app = null
  try {
    app = await connect(port)
    await app.waitFor(
      `document.readyState === "complete" && (document.body?.innerText?.trim().length ?? 0) > 0 && document.documentElement.clientWidth > 0 && document.documentElement.clientHeight > 0`,
      { timeout: 30000 },
    )
    const metrics = await app.evaluate(`({ width: document.documentElement.clientWidth, height: document.documentElement.clientHeight, textLength: document.body.innerText.trim().length })`)
    const shot = await app.send("Page.captureScreenshot", { format: "png" })
    const bytes = Buffer.from(shot.data, "base64")
    if (bytes.byteLength < 1024) throw new Error(`${name} screenshot unexpectedly small (${bytes.byteLength} bytes)`)
    await writeFile(screenshotPath, bytes)

    let logText = ""
    for (const check of requiredLogs) {
      logText = await waitForLog(logFile, check.pattern, check.label, 15000)
    }
    const protocolError = /Wayland protocol error|wl_display.*error/i.test(`${logText}\n${stderr}`)
    if (protocolError) throw new Error(`${name} logged a Wayland protocol error`)
    const exit = await stopChild(child, exited, app)
    return {
      name,
      passed: true,
      waylandDisplay: process.env.WAYLAND_DISPLAY,
      bwrapDri,
      metrics,
      screenshot: path.relative(root, screenshotPath),
      matchedLogs: requiredLogs.map((entry) => entry.label),
      exit,
    }
  } finally {
    await stopChild(child, exited, app).catch(() => {})
    await rm(profileRoot, { recursive: true, force: true }).catch(() => {})
  }
}

await mkdir(outDir, { recursive: true })
const phases = [
  {
    name: "wayland-dmabuf",
    bwrapDri: false,
    requiredLogs: [{ label: "DMA-BUF CEF frame", pattern: /received first Wayland DMA-BUF CEF frame/ }],
  },
  {
    name: "wayland-shm-fallback",
    bwrapDri: true,
    requiredLogs: [
      { label: "GBM unavailable fallback warning", pattern: /Wayland GBM DMA-BUF copier unavailable; falling back to CEF wl_shm painting/ },
      { label: "SHM CEF frame", pattern: /received first Wayland SHM CEF frame/ },
    ],
  },
]

const results = []
let failed = false
for (const phase of phases) {
  try {
    results.push(await runPhase(phase))
  } catch (error) {
    failed = true
    results.push({ name: phase.name, passed: false, error: error instanceof Error ? error.message : String(error) })
  }
}

const report = {
  checkedAt: new Date().toISOString(),
  account: options.account ?? null,
  waylandDisplay: process.env.WAYLAND_DISPLAY,
  passed: !failed,
  phases: results,
}
await writeFile(path.join(outDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`)
console.log(JSON.stringify(report, null, 2))
if (failed) process.exit(1)
