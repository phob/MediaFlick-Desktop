import { spawn } from "node:child_process"
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs"
import path from "node:path"
import { createInterface } from "node:readline"

const root = path.resolve(import.meta.dirname, "..")
const stageDir = path.join(root, "build")
const release = process.argv.includes("--release")
const runtimeLibrary = { linux: "libcef.so", win32: "libcef.dll" }[process.platform]

const cargo = spawn(
  "cargo",
  ["build", ...(release ? ["--release"] : []), "--bin", "mediaflick-desktop", "--message-format=json-render-diagnostics"],
  { cwd: root, stdio: ["ignore", "pipe", "inherit"] },
)

let binary = { executable: "", filenames: [] }
let cefLinkPaths = []
for await (const line of createInterface({ input: cargo.stdout })) {
  const message = JSON.parse(line)
  if (message.reason === "compiler-artifact" && message.target.name === "mediaflick-desktop" && message.target.kind.includes("bin")) {
    binary = message
  } else if (message.reason === "build-script-executed" && message.package_id.includes("#cef-dll-sys@")) {
    cefLinkPaths = message.linked_paths.map((entry) => entry.replace(/^native=/, ""))
  }
}
const code = await new Promise((resolve) => cargo.on("close", resolve))
if (code !== 0) process.exit(code ?? 1)
if (!binary.executable) throw new Error("cargo reported no mediaflick-desktop executable")

mkdirSync(stageDir, { recursive: true })
for (const file of binary.filenames) copyFileSync(file, path.join(stageDir, path.basename(file)))

if (runtimeLibrary) {
  const cefDir = cefLinkPaths.find((dir) => existsSync(path.join(dir, runtimeLibrary)))
  if (!cefDir) {
    throw new Error(`cef-dll-sys reported no CEF directory containing ${runtimeLibrary} (link paths: ${cefLinkPaths.join(", ") || "none"})`)
  }
  for (const entry of readdirSync(cefDir, { withFileTypes: true })) {
    if (entry.isFile()) copyFileSync(path.join(cefDir, entry.name), path.join(stageDir, entry.name))
  }
  rmSync(path.join(stageDir, "locales"), { recursive: true, force: true })
  cpSync(path.join(cefDir, "locales"), path.join(stageDir, "locales"), { recursive: true })
  console.log(`Staged ${path.basename(binary.executable)} and the CEF runtime from ${cefDir} in build/`)
} else {
  console.log(`Staged ${path.basename(binary.executable)} in build/`)
}
