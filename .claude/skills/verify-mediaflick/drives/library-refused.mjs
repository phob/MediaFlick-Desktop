// A library.db this build cannot use: it refuses it, leaves it as it was,
// tries only once, and says why on a startup error screen instead of the
// sign-in screen. `scripts/e2e/upgrade-0-2-0.mjs` runs it on a copy of a
// 0.2.0 profile, stamped with the next schema (scenario 4-newer-refused) or
// with schema 0 so the schema change fails and rolls back (scenario
// 5-migration-failed), with
//
//   --services-down --seed-data DIR
//   MEDIAFLICK_VERIFY_REFUSED_SHA256   the seeded library.db's SHA-256
//   MEDIAFLICK_VERIFY_REFUSED_SCHEMA   its PRAGMA user_version
//   MEDIAFLICK_VERIFY_REFUSED_REASON   JSON { says, patterns, leavesOut?, logKeeps? }:
//                                      what the reason must say, as regular
//                                      expression sources; what it must not
//                                      say, and what app.log keeps in full,
//                                      each as { says, patterns }
//
// The services stay down, so every API call answers 503 marked
// `startupFailed`, with the reason app.log gave. Several calls later app.log
// still has one `could not open` line: a failed open is not retried on each
// request. The main window is shown with the startup error screen holding
// that reason and no sign-in form, and library.db keeps its bytes and its
// version. The reason stays short enough to read: at most
// REASON_LIMIT characters, the profile's path included. Every check is in library-refused.json.
import { createHash } from "node:crypto"
import { readFileSync } from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"

const REASON_LIMIT = 600

function required(name) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set; run scripts/e2e/upgrade-0-2-0.mjs instead of this drive alone`)
  return value
}

export default async function libraryRefused(ctx) {
  const expectedSha = required("MEDIAFLICK_VERIFY_REFUSED_SHA256")
  const expectedSchema = Number(required("MEDIAFLICK_VERIFY_REFUSED_SCHEMA"))
  const expectedReason = JSON.parse(required("MEDIAFLICK_VERIFY_REFUSED_REASON"))
  const report = { checks: [] }
  const record = (ok, message, detail) => {
    report.checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
    ctx.step(`${ok ? "ok" : "FAILED"}: ${message}${detail === undefined ? "" : ` (${JSON.stringify(detail)})`}`)
  }
  try {
    const opens = () => (ctx.appLog().match(/could not open .*library\.db/g) ?? []).length
    await ctx.until(() => opens() > 0, "app.log to report the refused library.db", 30000)
    const answers = []
    for (let index = 0; index < 6; index += 1) answers.push(await ctx.api("/api/status"))
    report.answers = answers.map((answer) => ({ status: answer.status, body: answer.body }))
    record(answers.every((answer) => answer.status === 503), "every /api/status answers 503 while the services are down", answers.map((answer) => answer.status))
    // The path may hold spaces (it lives under TMPDIR). The reason runs to
    // the next log line: a failed schema change quotes its SQL over several
    // lines.
    const logged = ctx.appLog().match(/could not open [^\r\n]*?library\.db: [\s\S]*?(?=\n\d{4}-\d\d-\d\dT|\n?$)/)?.[0] ?? null
    const reason = answers[0].body?.error ?? ""
    const matches = (patterns, text) => patterns.every((pattern) => new RegExp(pattern).test(text))
    record(matches(expectedReason.patterns, reason), `the 503 says ${expectedReason.says}`, answers[0].body)
    record(reason.length <= REASON_LIMIT, `the reason is at most ${REASON_LIMIT} characters`, reason.length)
    if (expectedReason.leavesOut) {
      record(!expectedReason.leavesOut.patterns.some((pattern) => new RegExp(pattern).test(reason)), `the reason leaves out ${expectedReason.leavesOut.says}`)
    }
    if (expectedReason.logKeeps) {
      record(matches(expectedReason.logKeeps.patterns, ctx.appLog()), `app.log keeps ${expectedReason.logKeeps.says}`)
    }
    record(answers.every((answer) => answer.body?.startupFailed === true), "every 503 is marked as a failed startup", answers.map((answer) => answer.body?.startupFailed))
    record(logged !== null && reason === logged, "the 503 carries the reason app.log gave", logged)
    await ctx.until(() => ctx.windowShown(), "the main window to be shown", 30000)
    record(true, "the main window is shown")
    const screen = await ctx.until(
      `document.querySelector('[role="heading"][aria-level="1"]')?.textContent === "MediaFlick could not start"`,
      "the startup error screen",
      30000,
    ).then(() => true, () => false)
    await ctx.screenshot("01-refused")
    record(screen, "the startup error screen is shown")
    const text = await ctx.app.evaluate("document.body.innerText")
    // The page lays the reason out as text, which collapses its line breaks.
    const collapsed = (value) => value.replace(/\s+/g, " ").trim()
    record(logged !== null && collapsed(text).includes(collapsed(logged)), "the screen shows the reason app.log gave", text)
    record(!(await ctx.exists({ role: "button", name: "Sign in" })) && !text.includes("Welcome to MediaFlick"), "the sign-in screen is not shown")
    record(opens() === 1, "the open was tried once, not again for each request", opens())
    const file = path.join(ctx.configDir, "../../data/mediaflick-desktop/library.db")
    const sha = createHash("sha256").update(readFileSync(file)).digest("hex")
    record(sha === expectedSha, "library.db keeps its bytes", sha)
    const db = new DatabaseSync(file, { readOnly: true })
    try {
      const version = db.prepare("PRAGMA user_version").get().user_version
      record(version === expectedSchema, `library.db keeps schema ${expectedSchema}`, version)
    } finally {
      db.close()
    }
  } finally {
    ctx.writeEvidence("library-refused.json", report)
  }
  const failed = report.checks.filter((check) => !check.ok).map((check) => check.message)
  if (failed.length) throw new Error(`${failed.length} check(s) failed: ${failed.join("; ")}`)
}
