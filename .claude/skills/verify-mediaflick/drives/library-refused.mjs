// A library.db that a newer MediaFlick wrote: this build refuses it and leaves
// it as it was, and tries only once. `scripts/e2e/upgrade-0-2-0.mjs` runs it
// (scenario 4-newer-refused) on a copy of a 0.2.0 profile stamped with the
// next schema, with
//
//   --services-down --seed-data DIR
//   MEDIAFLICK_VERIFY_REFUSED_SHA256   the seeded library.db's SHA-256
//   MEDIAFLICK_VERIFY_REFUSED_SCHEMA   its PRAGMA user_version
//
// The services stay down, so every API call answers 503 with the reason: a
// newer MediaFlick wrote the file, which was left as it was. Several calls
// later app.log still has one `could not open` line: a failed open is not
// retried on each request. The main window is still shown, and library.db
// keeps its bytes and its version. Every check is in library-refused.json.
import { createHash } from "node:crypto"
import { readFileSync } from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"

function required(name) {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is not set; run scripts/e2e/upgrade-0-2-0.mjs instead of this drive alone`)
  return value
}

export default async function libraryRefused(ctx) {
  const expectedSha = required("MEDIAFLICK_VERIFY_REFUSED_SHA256")
  const expectedSchema = Number(required("MEDIAFLICK_VERIFY_REFUSED_SCHEMA"))
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
    const reason = JSON.stringify(answers[0].body ?? "")
    record(/newer MediaFlick/.test(reason) && /left as it was/.test(reason), "the 503 says a newer MediaFlick wrote library.db and that it was left as it was", answers[0].body)
    await ctx.until(() => ctx.windowShown(), "the main window to be shown", 30000)
    record(true, "the main window is shown")
    await ctx.screenshot("01-refused")
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
