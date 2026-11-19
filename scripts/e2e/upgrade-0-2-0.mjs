// End-to-end proof that updating from MediaFlick 0.2.0, the first release
// with a library.db, keeps a user's data, that the real 0.2.0 still runs on
// what this build left, and that this build then takes the profile back.
// Linux only.
//
//   just upgrade-e2e-0-2-0 --account https://jellyfin.example.com --profile DIR
//   node scripts/e2e/upgrade-0-2-0.mjs --account SERVER --profile DIR [--only id,id] [--negative-control NAME]
//
// --profile is a profile the released 0.2.0 AppImage left, in the verify
// layout (config/mediaflick-desktop, data/mediaflick-desktop), signed in to
// --account's cached session (`just verify-login SERVER`) on the same device.
// It holds a real server's library, so it never enters the repository; the
// sanitized part of one is frozen in fixtures/. Make one with the released
// AppImage as fixtures/library-db/README.md says.
//
// Each start of an app on the profile is checked in two steps:
//
// 1. The same binary first opens a copy of the profile with --library-stats,
//    which runs the real `Library::open`, so the migration, and exits before
//    any sync could refill a table. Every table of that copy must keep every
//    row, value for value, projected onto the columns it had (pre-sync.json).
// 2. The binary then starts on the profile itself through verify-mediaflick
//    (`drives/upgrade-0-2-0.mjs`), which checks the session, the user's files
//    and choices in the UI, and a completed sync.
//
// Scenarios: 1-update (this build on the 0.2.0 profile), 2-back (the real
// 0.2.0 on what 1-update left), 3-forward (this build again on what 2-back
// left), 4-newer-refused (this build on the profile stamped with a newer
// schema) and 5-migration-failed (stamped with schema 0, so the schema change
// fails and rolls back): this build must refuse the file once, leave it as it
// was, and show the reason on a startup error screen instead of sign-in. Every expectation comes from the profile as the previous app
// left it (before.json), never from the code under test. The artifact is
// build/e2e/upgrade-0-2-0/report.json, with each scenario's snapshots,
// screenshots and logs next to it. Exits non-zero when a scenario fails.
//
// --only picks scenarios; one that continues from another's profile brings it
// along. --negative-control NAME passes through to each session (see the verify
// skill's "Negative control"); pick the scenario with --only. Its drive must
// then fail on a check.
import { execFileSync, spawn } from "node:child_process"
import { createHash } from "node:crypto"
import { copyFileSync, cpSync, createReadStream, createWriteStream, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"
import { Readable } from "node:stream"
import { pipeline } from "node:stream/promises"
import { parseArgs } from "node:util"
import { readAccount, serverKey } from "../../.claude/skills/verify-mediaflick/scripts/account.mjs"
import { readDatabaseCopy, userFileDigests } from "../../.claude/skills/verify-mediaflick/scripts/profile-state.mjs"

const root = path.resolve(import.meta.dirname, "../..")
const verify = path.join(root, ".claude/skills/verify-mediaflick")
const out = path.join(root, "build/e2e/upgrade-0-2-0")
const RELEASED = {
  release: "v0.2.0",
  name: "MediaFlickDesktop-v0.2.0-linux-x86_64.AppImage",
  // The asset as published on 2026-11-17, matching GitHub's asset digest.
  sha256: "f10515096ab2e5fe34b8c4f8140a96b147c5fef3ae52cc85d8ffd2bcaac71772",
  schema: 23,
}
const FULLSCREEN_LABELS = { windowed: "Windowed", fullscreen: "Fullscreen" }

const { values: options } = parseArgs({
  options: {
    account: { type: "string" },
    profile: { type: "string" },
    only: { type: "string" },
    "negative-control": { type: "string" },
  },
})
if (process.platform !== "linux") throw new Error("the 0.2.0 upgrade drive runs on Linux; see `outstanding` in the report for Windows and macOS")
if (!options.account) throw new Error("--account SERVER is required: your own server, cached with `just verify-login SERVER`")
if (!options.profile) throw new Error("--profile DIR is required: a profile the released 0.2.0 left; see fixtures/library-db/README.md")
const account = readAccount(options.account)
if (!account) throw new Error(`no cached session for ${serverKey(options.account).normalized}; run \`just verify-login ${serverKey(options.account).normalized}\` first`)
const only = options.only ? new Set(options.only.split(",")) : null
const staged = path.join(root, "build/mediaflick-desktop")
if (!existsSync(staged)) throw new Error("build/mediaflick-desktop is missing; run `just build`")
const SCHEMA = Number(readFileSync(path.join(root, "src/library/db/migrate.rs"), "utf8").match(/^pub const SCHEMA_VERSION: i32 = (\d+);$/m)?.[1])
if (!SCHEMA) throw new Error("could not read SCHEMA_VERSION from src/library/db/migrate.rs")

const stamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15)
const sha256File = (file) => new Promise((resolve, reject) => {
  const digest = createHash("sha256")
  createReadStream(file).on("data", (chunk) => digest.update(chunk)).on("end", () => resolve(digest.digest("hex"))).on("error", reject)
})
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex")
const git = (...args) => execFileSync("git", ["-C", root, ...args], { encoding: "utf8" }).trim()
const json = (value) => `${JSON.stringify(value, null, 2)}\n`
const log = (line) => console.log(`[upgrade-0-2-0] ${line}`)
const appDirs = (profile) => ({ config: path.join(profile, "config/mediaflick-desktop"), data: path.join(profile, "data/mediaflick-desktop") })

const SCENARIOS = [
  {
    id: "1-update",
    title: "This build on a profile the released 0.2.0 left: nothing recreated, every row and choice kept",
    run: () => startOn("1-update", { binary: staged, profile: original, phase: "update", schema: SCHEMA }),
  },
  {
    id: "2-back",
    after: "1-update",
    title: "The way back: the real 0.2.0 on the profile this build left",
    run: async () => startOn("2-back", { binary: released, profile: profileOf("1-update"), phase: "back", schema: (await snapshot(profileOf("1-update"))).userVersion }),
  },
  {
    id: "3-forward",
    after: "2-back",
    title: "This build again on the profile 0.2.0 used after it",
    run: () => startOn("3-forward", { binary: staged, profile: profileOf("2-back"), phase: "forward", schema: SCHEMA }),
  },
  {
    id: "4-newer-refused",
    title: "A library.db from a newer MediaFlick: refused once, left as it was, the reason on a startup error screen",
    run: () => refusedStart("4-newer-refused", {
      schema: SCHEMA + 1,
      says: "a newer MediaFlick wrote library.db and that it was left as it was",
      patterns: ["newer MediaFlick", "left as it was"],
    }),
  },
  {
    id: "5-migration-failed",
    title: "A schema change that fails and rolls back: refused once, left as it was, the reason on a startup error screen",
    // No released step from 23 exists yet to fail, so the file claims schema
    // 0 while it holds 0.2.0's tables: creating the schema fails on the first
    // table that is already there, and the change rolls back.
    run: () => refusedStart("5-migration-failed", {
      schema: 0,
      says: "the step from schema 0 failed on SQLite's \"table credentials already exists\" and the file was left at schema 0",
      patterns: [`could not update library\\.db from schema 0 to ${SCHEMA}: table credentials already exists\\. The file was left at schema 0`],
      leavesOut: { says: "the statement SQLite ran", patterns: ["CREATE TABLE", " at offset \\d"] },
      logKeeps: { says: "the failed open in full, statement included", patterns: [`the failed open in full: could not update library\\.db from schema 0 to ${SCHEMA}: table credentials already exists in\\s+CREATE TABLE credentials \\(`] },
    }),
  },
]

// This build on a copy of the 0.2.0 profile whose library.db is stamped
// `schema`, which it must refuse once with a reason matching `patterns`,
// leave as it was, and show on the startup error screen.
async function refusedStart(id, { schema, says, patterns, leavesOut, logKeeps }) {
  const profile = copyProfile(original, path.join(out, id, "profile"))
  const { data } = appDirs(profile)
  writeDatabase(data, (db) => {
    db.exec(`PRAGMA user_version = ${schema}`)
    db.exec("PRAGMA wal_checkpoint(TRUNCATE)")
  })
  const file = path.join(data, "library.db")
  const bytes = await sha256File(file)
  const env = { ...process.env, HOME: path.join(profile, "home"), XDG_CONFIG_HOME: path.join(profile, "config"), XDG_DATA_HOME: path.join(profile, "data"), XDG_CACHE_HOME: path.join(profile, "cache"), XDG_STATE_HOME: path.join(profile, "state"), LD_LIBRARY_PATH: path.dirname(staged), ...(existsSync(path.join(path.dirname(staged), "libcef.so")) ? { LD_PRELOAD: path.join(path.dirname(staged), "libcef.so") } : {}) }
  const stats = spawnSync(staged, ["--library-stats"], env)
  const checks = [
    { ok: stats.code !== 0, message: "--library-stats refuses the library.db", detail: stats.code },
    { ok: patterns.every((pattern) => new RegExp(pattern).test(stats.stderr)), message: `the refusal says ${says}`, detail: stats.stderr.trim() },
    { ok: !/remove library\.db|delete library\.db/i.test(stats.stderr), message: "the refusal does not tell the user to remove library.db" },
    { ok: (await sha256File(file)) === bytes, message: "library.db is byte for byte as it was" },
  ]
  const run = await session(id, "library-refused", ["--services-down", "--seed-data", data], { MEDIAFLICK_VERIFY_REFUSED_SHA256: bytes, MEDIAFLICK_VERIFY_REFUSED_SCHEMA: String(schema), MEDIAFLICK_VERIFY_REFUSED_REASON: JSON.stringify({ says, patterns, leavesOut, logKeeps }) }, 600)
  const result = collect(id, run, "library-refused.json")
  rmSync(profile, { recursive: true, force: true })
  const passed = result.passed && checks.every((check) => check.ok)
  return { ...result, passed, checks: [...checks, ...(result.checks ?? [])], error: passed ? null : [result.error, ...checks.filter((check) => !check.ok).map((check) => check.message)].filter(Boolean).join("; ") }
}

const unknown = only ? [...only].filter((id) => !SCENARIOS.some((scenario) => scenario.id === id)) : []
if (unknown.length) throw new Error(`--only names no scenario ${unknown.join(", ")}; the scenarios are ${SCENARIOS.map((scenario) => scenario.id).join(", ")}`)
// A scenario that continues from another's profile brings that one along.
for (const scenario of [...SCENARIOS].reverse()) if (only?.has(scenario.id) && scenario.after) only.add(scenario.after)

rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })

async function releasedAppImage() {
  const dir = path.join(root, "build/e2e/mediaflick-0.2.0")
  const file = path.join(dir, RELEASED.name)
  if (!existsSync(file) || (await sha256File(file)) !== RELEASED.sha256) {
    mkdirSync(dir, { recursive: true })
    const url = `https://github.com/phob/MediaFlick-Desktop/releases/download/${RELEASED.release}/${RELEASED.name}`
    log(`downloading ${url}`)
    const reply = await fetch(url)
    if (!reply.ok) throw new Error(`${url}: HTTP ${reply.status}`)
    await pipeline(Readable.fromWeb(reply.body), createWriteStream(file))
    const digest = await sha256File(file)
    if (digest !== RELEASED.sha256) throw new Error(`${RELEASED.name} has SHA-256 ${digest}, not the published ${RELEASED.sha256}`)
  }
  execFileSync("chmod", ["+x", file])
  return file
}

// Opens library.db in place, to change a copy the script owns.
function writeDatabase(dataDir, work) {
  const db = new DatabaseSync(path.join(dataDir, "library.db"))
  try {
    return work(db)
  } finally {
    db.close()
  }
}

// Copies a profile's app dirs, with no token left in its library.db: each
// session puts the cached token back into its own disposable copy. The app's
// log is left behind, so each start's log holds only that start.
function copyProfile(from, to) {
  const source = appDirs(from)
  const target = appDirs(to)
  rmSync(to, { recursive: true, force: true })
  cpSync(source.config, target.config, { recursive: true, filter: (file) => !/mediaflick-desktop\.log(\.\d+)?$/.test(file) })
  cpSync(source.data, target.data, { recursive: true })
  writeDatabase(target.data, (db) => {
    // Overwrite the freed bytes too, or the token would stay in the file.
    db.exec("PRAGMA secure_delete = ON")
    db.exec("UPDATE credentials SET token = NULL WHERE id = 1")
    db.exec("PRAGMA wal_checkpoint(TRUNCATE)")
  })
  return to
}

// What the profile holds: library.db row by row, and the user's files.
async function snapshot(profile, columnsFrom = null) {
  const { config, data } = appDirs(profile)
  const database = await readDatabaseCopy(data, (db) => {
    const tables = {}
    const names = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").all().map((row) => row.name)
    const virtual = db.prepare("SELECT name FROM sqlite_master WHERE sql LIKE 'CREATE VIRTUAL TABLE%'").all().map((row) => row.name)
    for (const name of names.filter((table) => !virtual.some((owner) => table.startsWith(`${owner}_`)))) {
      const columns = db.prepare(`SELECT name FROM pragma_table_xinfo('${name}') WHERE hidden = 0`).all().map((row) => row.name)
      // Against an earlier snapshot, rows are compared on the columns that
      // snapshot had: a migration may add a column, never drop one.
      const held = columnsFrom?.tables[name]?.columns
      const compared = held && held.every((column) => columns.includes(column)) ? held : columns
      tables[name] = { columns, ...rowsDigest(db, name, compared) }
    }
    return {
      userVersion: db.prepare("PRAGMA user_version").get().user_version,
      credentials: db.prepare("SELECT server_url, server_id, user_id, user_name, device_id, updated_at, token IS NULL AS token_is_null FROM credentials WHERE id = 1").get(),
      meta: Object.fromEntries(db.prepare("SELECT key, value FROM meta ORDER BY key").all().map((row) => [row.key, row.value])),
      quickCheck: db.prepare("PRAGMA quick_check").all().map((row) => row.quick_check),
      foreignKeyCheck: db.prepare("PRAGMA foreign_key_check").all(),
      tables,
    }
  })
  const files = userFileDigests(config)
  const artDir = path.join(config, "custom-art")
  const customArt = existsSync(artDir) ? Object.fromEntries(readdirSync(artDir).sort().map((name) => [name, sha256(readFileSync(path.join(artDir, name)))])) : {}
  return { ...database, files, customArt, intent: intent(config, database.credentials, customArt) }
}

function rowsDigest(db, table, columns) {
  const list = columns.map((column) => `"${column}"`).join(", ")
  const rows = db.prepare(`SELECT ${list} FROM "${table}"`).all().map((row) => JSON.stringify(columns.map((column) => {
    const value = row[column]
    return value instanceof Uint8Array ? `blob:${sha256(value)}` : typeof value === "bigint" ? value.toString() : value
  }))).sort()
  return { rows: rows.length, digest: sha256(rows.join("\n")) }
}

// The choices the 0.2.0 user made, as the JSON files record them.
function intent(config, credentials, customArt) {
  const read = (name) => JSON.parse(readFileSync(path.join(config, name), "utf8"))
  const mine = (document) => document.accounts.find((entry) => entry.serverId === credentials.server_id && entry.userId === credentials.user_id)
  const settings = read("settings.json")
  const accounts = mine(read("accounts.json"))
  const collections = mine(read("collections.json"))
  const playback = mine(read("playback-preferences.json"))
  return {
    accent: accounts?.appearance?.accent ?? "signal",
    defaultFullscreen: FULLSCREEN_LABELS[settings.default_fullscreen ?? "fullscreen"],
    showScrollbars: settings.show_scrollbars ?? false,
    hideWatched: accounts?.viewing?.hideWatched ?? false,
    letterboxd: (accounts?.letterboxd_profiles ?? []).map((profile) => profile.displayName),
    collections: (collections?.profiles ?? []).map((profile) => {
      const poster = Object.keys(customArt).find((name) => profile.customPosterId && name.startsWith(`${profile.customPosterId}.`))
      return { title: profile.title, posterSha256: poster ? customArt[poster] : null }
    }),
    playback: (playback?.items ?? []).map((item) => item.subtitleTrack
      ? { itemId: item.itemId, select: "Subtitle track", shows: item.subtitleTrack.displayTitle }
      : { itemId: item.itemId, select: "Audio track", shows: item.audioTrack.displayTitle }),
  }
}

// Checks that `after` kept everything `before` had in library.db.
function compare(before, after, schema) {
  const checks = []
  const check = (ok, message, detail) => checks.push({ ok: Boolean(ok), message, ...(detail === undefined ? {} : { detail }) })
  check(after.userVersion === schema, `library.db is at schema ${schema}`, after.userVersion)
  check(after.quickCheck.length === 1 && after.quickCheck[0] === "ok", "quick_check is ok", after.quickCheck)
  check(after.foreignKeyCheck.length === 0, "no foreign key is broken", after.foreignKeyCheck)
  for (const [table, held] of Object.entries(before.tables)) {
    const now = after.tables[table]
    const columns = now && held.columns.every((column) => now.columns.includes(column))
    check(columns, `${table} keeps the columns it had`, columns ? undefined : now?.columns ?? "table gone")
    if (!columns) continue
    check(now.rows === held.rows && now.digest === held.digest, `${table} keeps all ${held.rows} rows, value for value`, now.rows === held.rows && now.digest === held.digest ? undefined : { before: held.rows, now: now.rows })
  }
  return checks
}

// One start of `binary` with --library-stats on a copy of `profile`: the real
// open, migration included, and an exit before any network or sync.
async function openBeforeSync(id, binary, profile, before, schema) {
  const dir = path.join(out, id, "pre-sync")
  const copy = copyProfile(profile, path.join(dir, "profile"))
  const env = {
    ...process.env,
    HOME: path.join(copy, "home"),
    XDG_CONFIG_HOME: path.join(copy, "config"),
    XDG_DATA_HOME: path.join(copy, "data"),
    XDG_CACHE_HOME: path.join(copy, "cache"),
    XDG_STATE_HOME: path.join(copy, "state"),
  }
  if (binary === staged) {
    env.LD_LIBRARY_PATH = [path.dirname(staged), process.env.LD_LIBRARY_PATH].filter(Boolean).join(":")
    const cef = path.join(path.dirname(staged), "libcef.so")
    if (existsSync(cef)) env.LD_PRELOAD = [cef, process.env.LD_PRELOAD].filter(Boolean).join(" ")
  }
  log(`${id}: ${path.basename(binary)} --library-stats on a copy`)
  const run = spawnSync(binary, ["--library-stats", "--log-level", "debug"], env)
  const appLog = readLog(appDirs(copy).config)
  writeFileSync(path.join(dir, "library-stats.log"), `exit ${run.code}\n--- stdout\n${run.stdout}\n--- stderr\n${run.stderr}\n--- mediaflick-desktop.log\n${appLog}`)
  const after = await snapshot(copy, before)
  const checks = [
    { ok: run.code === 0, message: `${path.basename(binary)} opens library.db (--library-stats exit ${run.code})`, ...(run.code === 0 ? {} : { detail: run.stderr.trim() }) },
    { ok: !/recreating the .*library database/.test(appLog), message: "the open recreates nothing" },
    ...compare(before, after, schema),
  ]
  const result = { binary: path.relative(root, binary), checks, after: summary(after) }
  writeFileSync(path.join(dir, "pre-sync.json"), json(result))
  rmSync(copy, { recursive: true, force: true })
  return { file: path.join(dir, "pre-sync.json"), ...result }
}

function spawnSync(binary, args, env) {
  try {
    const stdout = execFileSync(binary, args, { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 120000 })
    return { code: 0, stdout, stderr: "" }
  } catch (error) {
    return { code: error.status ?? -1, stdout: error.stdout ?? "", stderr: error.stderr ?? String(error) }
  }
}

function readLog(configDir) {
  const file = path.join(configDir, "mediaflick-desktop.log")
  return existsSync(file) ? readFileSync(file, "utf8") : ""
}

const summary = (snap) => ({ userVersion: snap.userVersion, rows: Object.fromEntries(Object.entries(snap.tables).map(([name, table]) => [name, table.rows])) })

function wrapped(command, env = {}, timeoutS = 900) {
  return new Promise((resolve) => {
    const child = spawn("bash", [path.join(verify, "scripts/isolate-linux.sh"), ...command], {
      cwd: root,
      env: { ...process.env, MEDIAFLICK_VERIFY_HARD_TIMEOUT: String(timeoutS), ...env },
      stdio: ["ignore", "pipe", "pipe"],
    })
    const chunks = []
    child.stdout.on("data", (chunk) => chunks.push(chunk))
    child.stderr.on("data", (chunk) => chunks.push(chunk))
    // Daemons the wrapper's session bus started can hold the pipes open after it exits.
    child.once("exit", (code) => {
      child.stdout.destroy()
      child.stderr.destroy()
      resolve({ code, output: Buffer.concat(chunks).toString("utf8") })
    })
  })
}

// One verify-mediaflick session; its evidence is build/verify/<run id>.
async function session(id, drive, args, env, timeoutS) {
  const runId = `upgrade-0-2-0-${id}-${stamp}`
  const control = options["negative-control"] ? ["--negative-control", options["negative-control"]] : []
  log(`${id}: just verify ${drive} ${[...args, ...control].join(" ")}`)
  const { code, output } = await wrapped(["node", path.join(verify, "scripts/session.mjs"), drive, "--run-id", runId, ...args, ...control], env, timeoutS)
  const evidence = path.join(root, "build/verify", runId)
  mkdirSync(evidence, { recursive: true })
  writeFileSync(path.join(evidence, "wrapper.log"), output)
  return { runId, code, evidence }
}

// A session's result for the report, with its evidence copied next to it.
function collect(id, run, checksFile) {
  const dir = path.join(out, id)
  mkdirSync(dir, { recursive: true })
  const result = existsSync(path.join(run.evidence, "result.json")) ? JSON.parse(readFileSync(path.join(run.evidence, "result.json"), "utf8")) : null
  const doctor = existsSync(path.join(run.evidence, "doctor.json")) ? JSON.parse(readFileSync(path.join(run.evidence, "doctor.json"), "utf8")) : null
  for (const name of readdirSync(run.evidence)) {
    if (/\.(png|json|log|txt|patch)$/.test(name)) copyFileSync(path.join(run.evidence, name), path.join(dir, name))
  }
  const checks = existsSync(path.join(run.evidence, checksFile)) ? JSON.parse(readFileSync(path.join(run.evidence, checksFile), "utf8")).checks : null
  return {
    runId: run.runId,
    evidence: path.relative(root, run.evidence),
    passed: run.code === 0 && result?.passed === true,
    negativeControl: result?.negativeControl ?? null,
    error: result?.error?.split("\n")[0] ?? (run.code === 0 ? null : `session exited ${run.code}`),
    exe: doctor?.exe ?? null,
    checks,
    screenshots: readdirSync(dir).filter((name) => name.endsWith(".png")).sort().map((name) => path.relative(root, path.join(dir, name))),
    log: existsSync(path.join(dir, "app.log")) ? path.relative(root, path.join(dir, "app.log")) : null,
  }
}

// Takes the profile a --keep-profile session left, without its token.
function keptProfile(run, to) {
  const kept = JSON.parse(readFileSync(path.join(run.evidence, "pids.json"), "utf8")).profile
  copyProfile(kept, to)
  rmSync(kept, { recursive: true, force: true })
  return to
}

const report = {
  generatedAt: new Date().toISOString(),
  git: { head: git("rev-parse", "HEAD"), branch: git("rev-parse", "--abbrev-ref", "HEAD"), dirty: git("status", "--porcelain") !== "" },
  schema: SCHEMA,
  builds: { staged: { path: path.relative(root, staged), modified: statSync(staged).mtime.toISOString(), sha256: await sha256File(staged) } },
  account: { server: account.server, userName: account.userName },
  negativeControl: options["negative-control"] ?? null,
  scenarios: [],
  outstanding: OUTSTANDING(),
  failures: [],
}
const released = await releasedAppImage()
report.builds.released = { release: RELEASED.release, path: path.relative(root, released), sha256: RELEASED.sha256 }

const original = copyProfile(path.resolve(options.profile), path.join(out, "0-profile-0.2.0"))
const start = await snapshot(original)
writeFileSync(path.join(out, "0-profile-0.2.0", "before.json"), json(start))
if (start.userVersion !== RELEASED.schema) throw new Error(`${options.profile} holds library.db at schema ${start.userVersion}, not 0.2.0's ${RELEASED.schema}`)
for (const [field, key] of [["server_id", "serverId"], ["user_id", "userId"], ["device_id", "deviceId"], ["server_url", "serverUrl"]]) {
  if (start.credentials?.[field] !== account[key]) throw new Error(`${options.profile} is signed in as another session than the one cached for ${account.server} (${field} differs)`)
}
report.profile = { source: path.resolve(options.profile), copy: path.relative(root, original), userVersion: start.userVersion, rows: summary(start).rows, intent: start.intent }

// One start of a binary on `profile`: the pre-sync open, then the UI session.
async function startOn(id, { binary, profile, phase, schema, args = [] }) {
  const before = await snapshot(profile)
  const beforeFile = path.join(out, id, "before.json")
  mkdirSync(path.dirname(beforeFile), { recursive: true })
  writeFileSync(beforeFile, json(before))
  const presync = await openBeforeSync(id, binary, profile, before, schema)
  const { config, data } = appDirs(profile)
  const env = {
    MEDIAFLICK_VERIFY_UPGRADE_BEFORE: beforeFile,
    MEDIAFLICK_VERIFY_UPGRADE_PRESYNC: presync.file,
    MEDIAFLICK_VERIFY_UPGRADE_PHASE: phase,
    MEDIAFLICK_VERIFY_UPGRADE_SCHEMA: String(schema),
    MEDIAFLICK_VERIFY_UPGRADE_LAUNCHED_AFTER: String(Math.floor(Date.now() / 1000)),
    ...(binary === staged ? {} : { MEDIAFLICK_DESKTOP_EXE: binary }),
  }
  const run = await session(id, "upgrade-0-2-0", ["--account", options.account, "--seed", config, "--seed-data", data, "--keep-profile", "--timeout", "900", ...args], env, 1500)
  const result = collect(id, run, "upgrade-0-2-0.json")
  const left = existsSync(path.join(run.evidence, "pids.json")) ? keptProfile(run, path.join(out, id, "profile")) : null
  return { ...result, presync: { binary: presync.binary, checks: presync.checks, after: presync.after }, before: summary(before), profile: left && path.relative(root, left) }
}

const results = {}
function profileOf(id) {
  const profile = results[id]?.profile
  if (!profile) throw new Error(`${id} left no profile to continue from`)
  return path.join(root, profile)
}

try {
  for (const scenario of SCENARIOS) {
    if (only && !only.has(scenario.id)) continue
    let result
    try {
      result = await scenario.run()
      if (result.presync && !result.presync.checks.every((check) => check.ok)) result.passed = false
    } catch (error) {
      result = { passed: false, error: error.message }
    }
    results[scenario.id] = result
    report.scenarios.push({ id: scenario.id, title: scenario.title, ...result })
    if (!result.passed) report.failures.push(`${scenario.id}: ${result.error ?? "failed"}`)
    log(`${scenario.id}: ${result.passed ? "passed" : `FAILED ${result.error ?? ""}`}`)
    writeFileSync(path.join(out, "report.json"), json(report))
  }
} finally {
  const leaks = scanForToken(out, account.token)
  report.tokenScan = leaks.length ? { ok: false, files: leaks } : { ok: true }
  if (leaks.length) report.failures.push(`the cached token is in ${leaks.join(", ")}`)
  writeFileSync(path.join(out, "report.json"), json(report))
}
log(`report: ${path.relative(root, path.join(out, "report.json"))}`)
for (const scenario of report.scenarios) log(`  ${scenario.passed ? "ok  " : "FAIL"} ${scenario.id}`)
if (report.failures.length) process.exitCode = 1

function scanForToken(dir, token) {
  const found = []
  const needle = Buffer.from(token)
  for (const entry of readdirSync(dir, { withFileTypes: true, recursive: true })) {
    if (!entry.isFile()) continue
    const file = path.join(entry.parentPath, entry.name)
    if (readFileSync(file).includes(needle)) found.push(path.relative(root, file))
  }
  return found
}

// What Linux cannot run, for someone on Windows or a Mac, step by step.
function OUTSTANDING() {
  return [
    {
      id: "windows-update-from-0.2.0",
      platform: "Windows",
      title: "Windows: this build's installer over an installed and signed-in 0.2.0",
      steps: [
        "Install MediaFlickDesktop-Setup-0.2.0.exe from the v0.2.0 release, sign in, let the library sync, set a choice on each settings shelf and add a My Collection with a custom poster.",
        "Close it and note the row counts of %LOCALAPPDATA%\\mediaflick-desktop\\library.db (sqlite3: SELECT count(*) per table).",
        "Install this build's installer over it and start it: it must start signed in, without `recreating the` in %APPDATA%\\mediaflick-desktop\\mediaflick-desktop.log, with the same counts before its first sync, and show every choice.",
        "Install 0.2.0 again over it and start it: signed in, Home renders, the choices show.",
      ],
    },
    {
      id: "macos-update-from-0.2.0",
      platform: "macOS",
      title: "macOS: this build's DMG over a signed-in 0.2.0",
      steps: [
        "Install the 0.2.0 DMG, sign in, let it sync, set choices as above, quit.",
        "Install this build's DMG over it, open it past Gatekeeper: signed in, no `recreating the` in ~/Library/Application Support/mediaflick-desktop/mediaflick-desktop.log, every choice shown.",
        "Install 0.2.0 again over it: signed in, Home renders.",
      ],
    },
  ]
}
