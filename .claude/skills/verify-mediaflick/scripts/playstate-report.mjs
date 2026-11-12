// Collects the playstate-proof cases from finished `just verify` runs into one
// report, per case and player:
//   node .claude/skills/verify-mediaflick/scripts/playstate-report.mjs <run-id>...
// Reads build/verify/<run-id>/ (result.json and the drive's own evidence) and
// writes build/e2e/playstate/report.json. Each case names the run it comes
// from; a case or player no run covered is listed as missing. Exits 1 unless
// every case passed or was skipped with a reason.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"

const root = path.resolve(import.meta.dirname, "../../../..")
const readJson = (file) => JSON.parse(readFileSync(file, "utf8"))
const player = (backend) => ({ libmpv: "builtin", builtin: "builtin", mpv: "external", external: "external" })[backend]

const CASES = [
  ["stop-mid", "Stop a film part way: Jellyfin holds that position"],
  ["resume", "Resume starts there; no report during the startup seek falls back"],
  ["play-to-end", "Play to the end: played, resume point cleared"],
  ["stop-early", "Stop within the first seconds: not played"],
  ["finish-episode", "Finish an episode: Next Up moves to the following one"],
  ["segment-skip", "Skip a segment: the reported position follows the skip"],
  ["audio-and-books", "A music queue, an audiobook position and a book position"],
  ["sign-out", "Sign out during playback: nothing reported to the other account"],
]

// Each drive's evidence, mapped to the cases it proves.
function casesOf(runId) {
  const dir = path.join(root, "build/verify", runId)
  const result = readJson(path.join(dir, "result.json"))
  const evidence = (name) => (existsSync(path.join(dir, name)) ? readJson(path.join(dir, name)) : null)
  const drive = path.basename(result.drive, ".mjs")
  const source = { runId, drive, runPassed: result.passed }
  const found = []
  if (drive === "playback") {
    const report = evidence("playback.json")
    const backend = player(report.backend)
    found.push({ name: "stop-mid", backend, passed: result.passed, movie: report.movie, server: report.stop, ...source })
    found.push({ name: "resume", backend, passed: result.passed, movie: report.movie, app: report.resume, server: report.cleanup, ...source })
  }
  if (drive === "playstate") {
    const report = evidence("playstate.json")
    for (const entry of report.cases) found.push({ backend: player(report.backend), ...entry, ...source })
  }
  if (drive === "segments") {
    const report = evidence("segments.json")
    const ask = report.cases.find((entry) => entry.name === "intro-ask")
    if (ask) {
      const checks = ask.checks.filter((check) => /Skip Intro jumped|Jellyfin's session follows the skip/.test(check.message))
      found.push({ name: "segment-skip", backend: player(report.backend), passed: checks.length === 2 && checks.every((check) => check.ok), skippedToMs: ask.skippedToMs, serverAfterSkipS: ask.serverAfterSkipS ?? null, checks, ...source })
    }
  }
  if (drive === "music") {
    const report = evidence("music.json")
    found.push({ name: "audio-and-books", part: "music and audiobook", backend: player(report.backend), passed: result.passed, session: report.session, audiobook: report.audiobook, ...source })
  }
  if (drive === "book-progress") {
    const report = evidence("book-progress.json")
    for (const backend of ["builtin", "external"]) found.push({ name: "audio-and-books", part: "book (the reader, not a player)", backend, passed: result.passed, final: report.final, ...source })
  }
  return found
}

const runIds = process.argv.slice(2)
if (!runIds.length) {
  console.error("usage: playstate-report.mjs <run-id>...")
  process.exit(2)
}
const found = runIds.flatMap(casesOf)
const report = { generatedAt: new Date().toISOString(), platform: process.platform, runs: runIds, cases: {} }
let complete = true
for (const [name, description] of CASES) {
  report.cases[name] = { description }
  for (const backend of ["builtin", "external"]) {
    const entries = found.filter((entry) => entry.name === name && entry.backend === backend)
    const status = !entries.length ? "missing" : entries.every((entry) => entry.skipped) ? "skipped" : entries.every((entry) => entry.skipped || entry.passed) ? "passed" : "failed"
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
