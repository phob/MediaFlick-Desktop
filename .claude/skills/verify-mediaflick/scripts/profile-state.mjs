import { createHash } from "node:crypto"
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { DatabaseSync, backup } from "node:sqlite"

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

export async function readDatabaseCopy(dataDir, read) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "mediaflick-profile-db-"))
  try {
    const source = new DatabaseSync(path.join(dataDir, "library.db"), { readOnly: true })
    try {
      await backup(source, path.join(dir, "library.db"))
    } finally {
      source.close()
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
