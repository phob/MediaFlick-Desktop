// What a profile holds, for the upgrade drives: digests of the files that
// hold a user's choices, and library.db read from a copy.
//
// `userFileDigests` covers the JSON stores and the custom posters. An upgrade
// drive compares them before and after an app starts, to prove starting
// rewrote none of them. accounts.json is digested without each account's `browsing`: the app saves
// the page it shows as `browsing.last` whenever it navigates, its first page
// at start included. That is navigation, not a change to anything the user
// chose.
import { createHash } from "node:crypto"
import { copyFileSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"

const STORES = ["settings.json", "accounts.json", "collections.json", "playback-preferences.json"]

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex")

function digest(name, bytes) {
  if (name !== "accounts.json") return sha256(bytes)
  const document = JSON.parse(bytes.toString("utf8"))
  for (const account of document.accounts ?? []) delete account.browsing
  return sha256(JSON.stringify(document))
}

export function userFileDigests(configDir) {
  const digests = {}
  for (const name of STORES) {
    const file = path.join(configDir, name)
    if (existsSync(file)) digests[name] = digest(name, readFileSync(file))
  }
  const artDir = path.join(configDir, "custom-art")
  if (existsSync(artDir)) {
    for (const name of readdirSync(artDir).sort()) digests[`custom-art/${name}`] = sha256(readFileSync(path.join(artDir, name)))
  }
  return digests
}

// Runs `read` on a copy of the library.db in `dataDir`: a read-only open of a
// WAL database still writes its -shm file, and the app may have it open.
export function readDatabaseCopy(dataDir, read) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "mediaflick-profile-db-"))
  try {
    for (const suffix of ["", "-wal", "-shm"]) {
      const file = path.join(dataDir, `library.db${suffix}`)
      if (existsSync(file)) copyFileSync(file, path.join(dir, `library.db${suffix}`))
    }
    const db = new DatabaseSync(path.join(dir, "library.db"))
    try {
      return read(db)
    } finally {
      db.close()
    }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}
