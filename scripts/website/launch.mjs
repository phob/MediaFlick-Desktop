// Starts the staged app (`just build`) in a throwaway profile so website
// captures never touch the real one.
//
//   node scripts/website/launch.mjs demo   # empty profile, Jellyfin's public demo server
//   node scripts/website/launch.mjs home   # copy of the current user's signed-in profile
//
// The copy takes the durable settings, library.db and the image cache; it
// leaves out instance.json so the capture app gets its own single-instance
// gate and can run beside a normal session. Profiles live under
// build/website-capture/profiles and are rebuilt from scratch on every launch.

import { spawn } from "node:child_process"
import { cp, mkdir, rm, readdir, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import path from "node:path"

export const PORTS = { demo: 9341, home: 9342 }
export const DEMO_URL = "https://demo.jellyfin.org/stable"

const root = path.resolve(import.meta.dirname, "../..")

export async function launch(profile) {
  if (!(profile in PORTS)) throw new Error(`unknown profile ${profile}; use demo or home`)
  const exe = path.join(root, "build", "mediaflick-desktop.exe")
  if (!existsSync(exe)) throw new Error("build/mediaflick-desktop.exe is missing; run `just build` first")

  const base = path.join(root, "build", "website-capture", "profiles", profile)
  const roaming = path.join(base, "Roaming")
  const local = path.join(base, "Local")
  await stop(profile)
  await rm(base, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 })
  await mkdir(path.join(roaming, "mediaflick-desktop"), { recursive: true })
  await mkdir(path.join(local, "mediaflick-desktop"), { recursive: true })

  const args = ["--remote-debugging-port", String(PORTS[profile]), "--log-level", "info"]
  if (profile === "demo") {
    args.push("--url", DEMO_URL)
    // Windowed playback keeps the player inside the capture window instead of
    // taking over the screen. Everything else stays at the defaults.
    const settings = { default_fullscreen: "windowed", webui_window: { width: 1600, height: 1000, maximized: false } }
    await writeFile(path.join(roaming, "mediaflick-desktop", "settings.json"), `${JSON.stringify(settings, null, 2)}\n`)
  }
  if (profile === "home") {
    const realRoaming = path.join(process.env.APPDATA ?? "", "mediaflick-desktop")
    const realLocal = path.join(process.env.LOCALAPPDATA ?? "", "mediaflick-desktop")
    for (const entry of await readdir(realRoaming)) {
      if (entry === "instance.json" || entry.endsWith(".log") || /\.log\.\d+$/.test(entry)) continue
      await cp(path.join(realRoaming, entry), path.join(roaming, "mediaflick-desktop", entry), { recursive: true })
    }
    for (const entry of ["library.db", "library.db-wal", "library.db-shm", "image-cache"]) {
      const source = path.join(realLocal, entry)
      if (existsSync(source)) await cp(source, path.join(local, "mediaflick-desktop", entry), { recursive: true })
    }
  }

  // `just build` does not stage libmpv (that needs `just libmpv` through WSL);
  // borrow the installed release's runtime for the built-in player.
  const env = { ...process.env, APPDATA: roaming, LOCALAPPDATA: local }
  const installedLibmpv = path.join(process.env.LOCALAPPDATA ?? "", "Programs", "MediaFlick Desktop", "libmpv-2.dll")
  if (!existsSync(path.join(root, "build", "libmpv-2.dll")) && existsSync(installedLibmpv)) {
    env.MEDIAFLICK_DESKTOP_LIBMPV_PATH = installedLibmpv
  }

  const child = spawn(exe, args, {
    cwd: path.dirname(exe),
    env,
    detached: true,
    stdio: "ignore",
  })
  child.unref()
  return { pid: child.pid, port: PORTS[profile] }
}

// The capture app is the only mediaflick-desktop process listening on the
// profile's debugging port; a normal session never passes that flag.
export async function stop(profile) {
  const port = PORTS[profile]
  const script = `Get-CimInstance Win32_Process -Filter "Name = 'mediaflick-desktop.exe'" | Where-Object { $_.CommandLine -match '--remote-debugging-port[ =]${port}\\b' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`
  await new Promise((resolve) => {
    spawn("pwsh.exe", ["-NoLogo", "-NoProfile", "-Command", script], { stdio: "ignore" }).on("exit", resolve)
  })
  await new Promise((resolve) => setTimeout(resolve, 1000))
}

if (import.meta.url === `file:///${process.argv[1].replaceAll("\\", "/")}`) {
  const [profile = "demo", action = "start"] = process.argv.slice(2)
  if (action === "stop") await stop(profile)
  else console.log(JSON.stringify(await launch(profile)))
}
