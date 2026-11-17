// Custom posters survive a start whose collections.json is damaged and has no
// valid backup. Such a start moves the file aside as
// collections.json.broken-<time> and opens with no collections, so the only
// record of which posters are in use is the moved-aside file. The orphaned
// poster cleanup that follows must not delete them. Bug report (PR #338
// follow-up): before the fix that start deleted every custom poster older than
// 7 days.
//
// The drive starts on a fresh profile, then writes a collections.json cut off
// mid-write that names two posters, no collections.json.bak, and the two
// posters dated 30 days back (the harness seed copy does not keep mtimes, so
// the drive writes them itself). It relaunches the app on that profile and
// checks that the damaged file was moved aside byte for byte and both posters
// are still there byte for byte. Then it relaunches once more: there is no
// collections.json by then, only the moved-aside file, and both posters must
// still survive (operator decision on PR #342: skip the cleanup while any
// collections.json.broken-* exists). Last it removes the moved-aside file and
// relaunches a third time: with no collections.json, no .bak and no .broken
// copy, nothing names the posters, and a missing collections.json must still
// delete nothing (operator decision of 2026-11-17: a user who moved or deleted
// the file, or a release that renames it, must not lose every poster). Before
// that fix this start deleted both. Never signs in. Every check is recorded in
// poster-cleanup.json and the run fails at the end listing each failed one.
//   just verify poster-cleanup
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs"
import path from "node:path"

const DAY_MS = 24 * 60 * 60 * 1000
const POSTERS = [
  { id: "4f1c2a9e7b3d5c8a0e6f1b2d3c4a5e6f", extension: "png", bytes: Buffer.from("\x89PNG\r\n\x1a\nposter-cleanup-png", "latin1") },
  { id: "9a8b7c6d5e4f30211203f4e5d6c7b8a9", extension: "jpg", bytes: Buffer.from("\xff\xd8\xffposter-cleanup-jpg", "latin1") },
]

// A collections.json cut off mid-write: no readable version, so the app
// treats it as damaged rather than as another version's file.
function damagedCollections() {
  const profile = (poster, index) => `{"id":"profile${index}","revision":"r1","title":"Poster ${index}","description":"","customPosterId":"${poster.id}"`
  return `{"version":1,"accounts":[{"serverId":"server","userId":"user","profiles":[${POSTERS.map(profile).join("},")}`
}

export default async function posterCleanup(ctx) {
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }

  try {
    await steps(ctx, report, record)
  } finally {
    ctx.writeEvidence("poster-cleanup.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}

async function steps(ctx, report, record) {
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen", 30000)
  const artDir = ctx.configFile("custom-art")
  const collections = ctx.configFile("collections.json")
  const damaged = damagedCollections()
  const aged = new Date(Date.now() - 30 * DAY_MS)
  mkdirSync(artDir, { recursive: true })
  writeFileSync(collections, damaged)
  for (const poster of POSTERS) {
    const file = path.join(artDir, `${poster.id}.${poster.extension}`)
    writeFileSync(file, poster.bytes)
    utimesSync(file, aged, aged)
  }
  report.seeded = {
    collections: damaged,
    posters: POSTERS.map((poster) => ({ file: `${poster.id}.${poster.extension}`, mtime: statSync(path.join(artDir, `${poster.id}.${poster.extension}`)).mtime.toISOString() })),
  }
  record(!existsSync(`${collections}.bak`), "the profile has no collections.json.bak to recover from")

  await ctx.relaunch()
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen after relaunch", 30000)
  await ctx.screenshot("01-after-relaunch")

  const configNames = readdirSync(ctx.configDir)
  const broken = configNames.filter((name) => name.startsWith("collections.json.broken-"))
  report.configFiles = configNames.sort()
  record(broken.length === 1, "the damaged collections.json was moved aside once", broken)
  record(broken.length === 1 && readFileSync(path.join(ctx.configDir, broken[0]), "utf8") === damaged, "the moved-aside file keeps the damaged bytes")

  report.postersAfterDamagedStart = checkPosters(artDir, record, "named only by the damaged file", "the damaged start")

  await ctx.relaunch()
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen after the second relaunch", 30000)
  await ctx.screenshot("02-after-second-relaunch")
  const secondNames = readdirSync(ctx.configDir)
  report.configFilesAfterSecondStart = secondNames.sort()
  record(!secondNames.includes("collections.json"), "the next start has no collections.json")
  record(broken.length === 1 && secondNames.includes(broken[0]), "the moved-aside file is still there on the next start", broken)
  report.postersAfterNextStart = checkPosters(artDir, record, "named only by the damaged file", "the next start")

  for (const name of broken) rmSync(path.join(ctx.configDir, name))
  await ctx.relaunch()
  await ctx.until("document.title === 'Sign in — MediaFlick'", "the sign-in screen after the third relaunch", 30000)
  await ctx.screenshot("03-after-missing-collections")
  const thirdNames = readdirSync(ctx.configDir)
  report.configFilesAfterMissingCollections = thirdNames.sort()
  record(!thirdNames.some((name) => name.startsWith("collections.json")), "the third start has no collections.json, .bak or .broken copy", thirdNames)
  report.postersAfterMissingCollections = checkPosters(artDir, record, "named by no file", "a start with no collections.json")
}

function checkPosters(artDir, record, namedBy, start) {
  for (const poster of POSTERS) {
    const name = `${poster.id}.${poster.extension}`
    const file = path.join(artDir, name)
    const kept = existsSync(file)
    record(kept, `poster ${name}, 30 days old and ${namedBy}, survives ${start}`)
    record(kept && readFileSync(file).equals(poster.bytes), `poster ${name} keeps its bytes after ${start}`)
  }
  return existsSync(artDir) ? readdirSync(artDir).sort() : []
}
