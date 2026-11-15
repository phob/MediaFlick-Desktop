// Builds mediaflick-desktop and stages it with the CEF runtime in build/, the
// directory `just run`, `just verify` and the release packagers start from.
// Usage: node scripts/stage-app.mjs [--release]
//
// cef-dll-sys copies the runtime "next to the binary" by taking the third
// parent of its OUT_DIR as the target directory. Cargo 1.100's build-dir layout
// moved OUT_DIR one level deeper, so those copies now land in <profile>/build/.
// Cargo's JSON messages name both inputs instead of a guessed path: the
// binary's artifact files, and the CEF directory cef-dll-sys hands the linker.
import { spawn } from "node:child_process"
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs"
import path from "node:path"
import { createInterface } from "node:readline"

const root = path.resolve(import.meta.dirname, "..")
const stageDir = path.join(root, "build")
const release = process.argv.includes("--release")
// cef-dll-sys puts the runtime beside the binary only on Linux and Windows; the
// macOS DMG script takes the framework from the CEF directory itself.
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
// The executable and, on Windows, its debug symbols.
for (const file of binary.filenames) copyFileSync(file, path.join(stageDir, path.basename(file)))

if (runtimeLibrary) {
  const cefDir = cefLinkPaths.find((dir) => existsSync(path.join(dir, runtimeLibrary)))
  if (!cefDir) {
    throw new Error(`cef-dll-sys reported no CEF directory containing ${runtimeLibrary} (link paths: ${cefLinkPaths.join(", ") || "none"})`)
  }
  // The set cef-dll-sys itself copies: the distribution's top-level files and its locales.
  for (const entry of readdirSync(cefDir, { withFileTypes: true })) {
    if (entry.isFile()) copyFileSync(path.join(cefDir, entry.name), path.join(stageDir, entry.name))
  }
  rmSync(path.join(stageDir, "locales"), { recursive: true, force: true })
  cpSync(path.join(cefDir, "locales"), path.join(stageDir, "locales"), { recursive: true })
  console.log(`Staged ${path.basename(binary.executable)} and the CEF runtime from ${cefDir} in build/`)
} else {
  console.log(`Staged ${path.basename(binary.executable)} in build/`)
}
