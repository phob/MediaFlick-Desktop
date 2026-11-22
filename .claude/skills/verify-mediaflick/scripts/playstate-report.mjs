import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"

const root = path.resolve(import.meta.dirname, "../../../..")
const readJson = (file) => JSON.parse(readFileSync(file, "utf8"))
const player = (backend) => ({ libmpv: "builtin", builtin: "builtin", mpv: "external", external: "external" })[backend]
const PLAYERS = ["builtin", "external"]
const MUSIC = "music and audiobook"
const BOOK = "book (the reader, not a player)"

const CASES = [
  ["stop-mid", "Stop a film part way: Jellyfin holds that position"],
  ["resume", "Resume starts there; no report during the startup seek falls back"],
  ["play-to-end", "Play to the end: played, resume point cleared"],
  ["stop-early", "Stop within the first seconds: not played"],
  ["finish-episode", "Finish an episode: Next Up moves to the following one"],
  ["segment-skip", "Skip a segment: the reported position follows the skip"],
  ["audio-and-books", "A music queue, an audiobook position and a book position", [MUSIC, BOOK]],
  ["sign-out", "Sign out during playback: nothing reported to the other account"],
]

const DRIVE_CASES = {
  playback: [{ name: "stop-mid" }, { name: "resume" }],
  playstate: [{ name: "play-to-end" }, { name: "finish-episode" }, { name: "stop-early" }, { name: "sign-out" }],
  segments: [{ name: "segment-skip" }],
  music: [{ name: "audio-and-books", part: MUSIC }],
  "book-progress": [{ name: "audio-and-books", part: BOOK, players: PLAYERS }],
}

function casesOf(runId) {
  const dir = path.join(root, "build/verify", runId)
  const result = readJson(path.join(dir, "result.json"))
  const drive = path.basename(result.drive, ".mjs")
  const source = { runId, drive, runPassed: result.passed }
  const file = { playback: "playback.json", playstate: "playstate.json", segments: "segments.json", music: "music.json", "book-progress": "book-progress.json" }[drive]
  if (!file) return []
  if (!existsSync(path.join(dir, file))) {
    const settings = path.join(dir, "profile/config/settings.json")
    const backend = existsSync(settings) && readJson(settings).player_backend === "mpv" ? "external" : "builtin"
    const error = `the run wrote no ${file}: ${result.error?.split("\n")[0] ?? "no error recorded"}`
    return DRIVE_CASES[drive].flatMap(({ name, part, players = [backend] }) => players.map((entry) => ({ name, part, backend: entry, passed: false, error, ...source })))
  }
  const report = readJson(path.join(dir, file))
  const found = []
  if (drive === "playback") {
    const backend = player(report.backend)
    found.push({ name: "stop-mid", backend, passed: result.passed, movie: report.movie, server: report.stop, ...source })
    found.push({ name: "resume", backend, passed: result.passed, movie: report.movie, app: report.resume, server: report.cleanup, ...source })
  }
  if (drive === "playstate") {
    for (const entry of report.cases) found.push({ backend: player(report.backend), ...entry, ...source })
  }
  if (drive === "segments") {
    const ask = report.cases.find((entry) => entry.name === "intro-ask")
    if (ask) {
      const checks = ask.checks.filter((check) => /Skip Intro jumped|Jellyfin's session follows the skip/.test(check.message))
      found.push({ name: "segment-skip", backend: player(report.backend), passed: checks.length === 2 && checks.every((check) => check.ok), skippedToMs: ask.skippedToMs, serverAfterSkipS: ask.serverAfterSkipS ?? null, checks, ...source })
    }
  }
  if (drive === "music") {
    found.push({ name: "audio-and-books", part: MUSIC, backend: player(report.backend), passed: result.passed, session: report.session, audiobook: report.audiobook, ...source })
  }
  if (drive === "book-progress") {
    for (const backend of PLAYERS) found.push({ name: "audio-and-books", part: BOOK, backend, passed: result.passed, final: report.final, ...source })
  }
  return found
}

function statusOf(entries, parts = []) {
  if (!entries.length) return "missing"
  if (entries.some((entry) => !entry.skipped && !entry.passed)) return "failed"
  if (parts.some((part) => !entries.some((entry) => entry.part === part))) return "missing"
  return entries.every((entry) => entry.skipped) ? "skipped" : "passed"
}

const runIds = process.argv.slice(2)
if (!runIds.length) {
  console.error("usage: playstate-report.mjs <run-id>...")
  process.exit(2)
}
const found = runIds.flatMap(casesOf)
const report = { generatedAt: new Date().toISOString(), platform: process.platform, runs: runIds, cases: {} }
let complete = true
for (const [name, description, parts] of CASES) {
  report.cases[name] = { description, ...(parts ? { parts } : {}) }
  for (const backend of PLAYERS) {
    const entries = found.filter((entry) => entry.name === name && entry.backend === backend)
    const status = statusOf(entries, parts)
    if (status === "missing" || status === "failed") complete = false
    report.cases[name][backend] = { status, entries }
  }
}
const out = path.join(root, "build/e2e/playstate")
mkdirSync(out, { recursive: true })
writeFileSync(path.join(out, "report.json"), `${JSON.stringify(report, null, 2)}\n`)
for (const [name] of CASES) console.log(`${name.padEnd(16)} built-in ${report.cases[name].builtin.status.padEnd(8)} external ${report.cases[name].external.status}`)
console.log(`wrote ${path.relative(root, path.join(out, "report.json"))}`)
process.exit(complete ? 0 : 1)
