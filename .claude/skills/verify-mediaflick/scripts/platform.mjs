// Every OS-specific detail of verify-mediaflick lives here, next to the two
// isolation wrappers (isolate-windows.ps1, isolate-linux.sh). session.mjs,
// harness.mjs and the drives are platform-neutral.

import { execFileSync } from "node:child_process"
import { existsSync, readFileSync, readdirSync, readlinkSync } from "node:fs"
import os from "node:os"
import path from "node:path"

export const root = path.resolve(import.meta.dirname, "../../../..")
export const isWindows = process.platform === "win32"
export const isLinux = process.platform === "linux"
if (!isWindows && !isLinux) throw new Error(`verify-mediaflick supports Windows and Linux, not ${process.platform}`)

export const buildDir = path.join(root, "build")
export const exe = path.join(buildDir, isWindows ? "mediaflick-desktop.exe" : "mediaflick-desktop")

// Every disposable profile lives under the OS temp dir with this prefix. It is
// also how leftovers are recognized: CEF child processes carry
// --user-data-dir=<profile>, and nothing else on the machine uses the prefix.
export const PROFILE_PREFIX = "mediaflick-verify-"
export const profileParent = os.tmpdir()

// Where the user's real profile lives, for the read-only guard. Resolved from
// this process's environment before any override, the same way the app does
// (src/app/paths.rs).
export function realProfileDirs(env = process.env) {
  if (isWindows) {
    const roaming = env.APPDATA ?? path.join(env.USERPROFILE ?? os.homedir(), "AppData", "Roaming")
    const local = env.LOCALAPPDATA ?? roaming
    return { config: path.join(roaming, "mediaflick-desktop"), data: path.join(local, "mediaflick-desktop") }
  }
  const home = env.HOME ?? os.homedir()
  return {
    config: path.join(env.XDG_CONFIG_HOME || path.join(home, ".config"), "mediaflick-desktop"),
    data: path.join(env.XDG_DATA_HOME || path.join(home, ".local", "share"), "mediaflick-desktop"),
  }
}

// Environment that points every per-user location the app (and CEF) uses at
// the disposable profile. Returns the env plus the app's config and data dirs.
export function appEnvironment(profile) {
  const env = { ...process.env }
  let configRoot
  let dataRoot
  if (isWindows) {
    configRoot = path.join(profile, "Roaming")
    dataRoot = path.join(profile, "Local")
    Object.assign(env, { APPDATA: configRoot, LOCALAPPDATA: dataRoot, TEMP: path.join(profile, "Temp"), TMP: path.join(profile, "Temp") })
  } else {
    configRoot = path.join(profile, "config")
    dataRoot = path.join(profile, "data")
    Object.assign(env, {
      HOME: path.join(profile, "home"),
      XDG_CONFIG_HOME: configRoot,
      XDG_DATA_HOME: dataRoot,
      XDG_CACHE_HOME: path.join(profile, "cache"),
      XDG_STATE_HOME: path.join(profile, "state"),
      TMPDIR: path.join(profile, "tmp"),
    })
    // Same loader setup as `just run` on Linux.
    const cef = path.join(buildDir, "libcef.so")
    env.LD_LIBRARY_PATH = [buildDir, process.env.LD_LIBRARY_PATH].filter(Boolean).join(":")
    if (existsSync(cef)) {
      env.MEDIAFLICK_DESKTOP_CEF_PRELOAD = cef
      env.LD_PRELOAD = [cef, process.env.LD_PRELOAD].filter(Boolean).join(" ")
    }
  }
  // Settings come from the profile, not from a developer's shell.
  for (const name of ["MEDIAFLICK_DESKTOP_REMOTE_DEBUGGING_PORT", "MEDIAFLICK_DESKTOP_LOG_FILE", "MEDIAFLICK_DESKTOP_LOG_LEVEL", "JELLYFIN_URL"]) delete env[name]
  const dirs = Object.values(isWindows ? { configRoot, dataRoot, temp: env.TEMP } : { configRoot, dataRoot, home: env.HOME, cache: env.XDG_CACHE_HOME, state: env.XDG_STATE_HOME, tmp: env.TMPDIR })
  return {
    env,
    dirs,
    configDir: path.join(configRoot, "mediaflick-desktop"),
    dataDir: path.join(dataRoot, "mediaflick-desktop"),
  }
}

// The MSI PowerShell 7 that the justfile's windows-shell pins. A Store
// (MSIX) pwsh found first on PATH runs children outside the verify job.
export const pwshPath = path.join(process.env.ProgramFiles ?? "C:\\Program Files", "PowerShell", "7", "pwsh.exe")

function pwsh(script) {
  return execFileSync(pwshPath, ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8", windowsHide: true })
}

// Confirms this process really runs inside the isolation the wrapper claims.
// Throws otherwise; nothing may be launched outside it.
export function assertIsolated() {
  const claim = process.env.MEDIAFLICK_VERIFY_ISOLATION ?? ""
  if (isWindows) {
    const expected = claim.startsWith("windows-desktop:") ? claim.slice("windows-desktop:".length) : ""
    if (!expected) throw new Error("not started by isolate-windows.ps1; run `just verify <drive>`")
    // A child pwsh inherits this process's desktop, so it reports ours.
    const [current, input] = pwsh(`Add-Type -Namespace MfVerify -Name Desk -MemberDefinition '
      [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
      [DllImport("user32.dll")] public static extern IntPtr GetThreadDesktop(uint id);
      [DllImport("user32.dll")] public static extern IntPtr OpenInputDesktop(uint f, bool i, uint a);
      [DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr d);
      [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern bool GetUserObjectInformation(IntPtr h, int i, System.Text.StringBuilder v, int s, out uint n);
      public static string Name(IntPtr d) { var t = new System.Text.StringBuilder(256); uint n; return d != IntPtr.Zero && GetUserObjectInformation(d, 2, t, 512, out n) ? t.ToString() : ""; }'
      $in = [MfVerify.Desk]::OpenInputDesktop(0, $false, 1)
      [MfVerify.Desk]::Name([MfVerify.Desk]::GetThreadDesktop([MfVerify.Desk]::GetCurrentThreadId()))
      [MfVerify.Desk]::Name($in)
      [void][MfVerify.Desk]::CloseDesktop($in)`).trim().split(/\r?\n/).map((line) => line.trim())
    if (current !== expected) throw new Error(`expected private desktop ${expected}, running on ${current || "unknown"}`)
    // The input desktop is the one the user sees (Default, or Winlogon when locked).
    if (current === input) throw new Error(`desktop ${current} is the visible input desktop; refusing to run`)
    return { kind: "windows-desktop", desktop: current, inputDesktop: input || "(unreadable: workstation locked?)" }
  }
  const display = claim.startsWith("linux-xvfb:") ? claim.slice("linux-xvfb:".length) : ""
  if (!display) throw new Error("not started by isolate-linux.sh; run `just verify <drive>`")
  if (process.env.DISPLAY !== display) throw new Error(`DISPLAY is ${process.env.DISPLAY}, expected ${display}`)
  if (process.env.WAYLAND_DISPLAY) throw new Error("WAYLAND_DISPLAY is set; the app would open on the user's Wayland session")
  if (!process.env.XDG_RUNTIME_DIR?.includes("mediaflick-verify-xvfb.")) throw new Error(`XDG_RUNTIME_DIR ${process.env.XDG_RUNTIME_DIR} is not the wrapper's private runtime dir`)
  const xvfb = Number(process.env.MEDIAFLICK_VERIFY_XVFB_PID)
  let xvfbExe = ""
  try {
    xvfbExe = path.basename(readlinkSync(`/proc/${xvfb}/exe`))
  } catch {
    // Reported below.
  }
  if (xvfbExe !== "Xvfb") throw new Error(`DISPLAY ${display} is not served by the wrapper's Xvfb (pid ${xvfb}: ${xvfbExe || "missing"})`)
  return { kind: "linux-xvfb", display, xvfbPid: xvfb, runtimeDir: process.env.XDG_RUNTIME_DIR, dbus: process.env.DBUS_SESSION_BUS_ADDRESS ? "private" : "none" }
}

// All processes as { pid, ppid, name, cmd }.
export function listProcesses() {
  if (isWindows) {
    const json = pwsh(`@(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine) | ConvertTo-Json -Compress -Depth 2`)
    const rows = JSON.parse(json || "[]")
    return (Array.isArray(rows) ? rows : [rows]).map((row) => ({ pid: row.ProcessId, ppid: row.ParentProcessId, name: row.Name ?? "", cmd: row.CommandLine ?? "" }))
  }
  const rows = []
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue
    try {
      const cmd = readFileSync(`/proc/${entry}/cmdline`, "utf8").split("\0").filter(Boolean).join(" ")
      const stat = readFileSync(`/proc/${entry}/stat`, "utf8")
      const name = stat.slice(stat.indexOf("(") + 1, stat.lastIndexOf(")"))
      const ppid = Number(stat.slice(stat.lastIndexOf(")") + 2).split(" ")[1])
      rows.push({ pid: Number(entry), ppid, name, cmd })
    } catch {
      // The process exited while listing.
    }
  }
  return rows
}

export const isAppProcess = (row) => /^mediaflick-desktop(\.exe)?$/i.test(row.name)

// Stops exactly the given PIDs and, with tree, the processes they started.
export function killPids(pids, { tree = false } = {}) {
  let targets = [...pids]
  if (tree && isLinux) {
    // Children first found through their parent PIDs; the app stays in the
    // wrapper's process group, so a group kill there remains the safety net.
    const rows = listProcesses()
    for (let index = 0; index < targets.length; index++) {
      for (const row of rows) if (row.ppid === targets[index] && !targets.includes(row.pid)) targets.push(row.pid)
    }
  }
  for (const pid of targets) {
    try {
      if (isWindows) execFileSync("taskkill.exe", ["/PID", String(pid), ...(tree ? ["/T"] : []), "/F"], { stdio: "ignore", windowsHide: true })
      else process.kill(pid, "SIGKILL")
    } catch {
      // Already gone.
    }
  }
}
