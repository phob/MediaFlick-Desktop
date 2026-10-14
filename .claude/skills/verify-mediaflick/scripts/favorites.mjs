// Favorites a drive adds on the cached account's server. Each one is
// journaled on disk before the drive writes it and dropped only once the
// server no longer lists it, so a run killed before its after-run cleanup
// leaves a record that the next `--account` run reconciles before launch.
// Entries hold server, user id and item id, never the token.

import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs"
import path from "node:path"
import { jellyfinGet, removeFavorite } from "./account.mjs"
import { buildDir } from "./platform.mjs"

export const journalFile = path.join(buildDir, "verify", "pending-favorites.json")

const FIELDS = ["server", "userId", "itemId"]

export function readJournal() {
  if (!existsSync(journalFile)) return []
  let entries = null
  try {
    entries = JSON.parse(readFileSync(journalFile, "utf8"))
  } catch {}
  if (!Array.isArray(entries) || !entries.every((entry) => FIELDS.every((field) => typeof entry?.[field] === "string" && entry[field]))) {
    throw new Error(`${journalFile} is not a JSON list of {server, userId, itemId}; check those favorites on the server, then fix or remove the file`)
  }
  return entries
}

function writeJournal(entries) {
  mkdirSync(path.dirname(journalFile), { recursive: true })
  const temporary = `${journalFile}.${process.pid}.tmp`
  writeFileSync(temporary, `${JSON.stringify(entries, null, 2)}
`)
  renameSync(temporary, journalFile)
}

const isEntry = (entry, account, itemId) => entry.server === account.server && entry.userId === account.userId && entry.itemId === itemId

export function journalFavorite(account, itemId) {
  const entries = readJournal()
  if (!entries.some((entry) => isEntry(entry, account, itemId))) writeJournal([...entries, { server: account.server, userId: account.userId, itemId }])
}

// `get` is a GET on the server as the account: `ctx.jellyfin` in a drive.
export async function isFavorite(get, userId, itemId) {
  return (await get(`/Items/${encodeURIComponent(itemId)}?userId=${userId}`)).UserData.IsFavorite === true
}

// Returns "removed", "not-favorite", or "missing" when the server answers 404
// for the item (a rename or an upgraded file can give a title a new id).
export async function settleFavorite(account, itemId) {
  let state
  try {
    state = (await isFavorite((apiPath) => jellyfinGet(account, apiPath), account.userId, itemId)) ? "removed" : "not-favorite"
  } catch (error) {
    if (error.status !== 404) throw error
    state = "missing"
  }
  if (state === "removed") await removeFavorite(account, itemId)
  writeJournal(readJournal().filter((entry) => !isEntry(entry, account, itemId)))
  return state
}

const RECONCILED = {
  removed: (itemId) => `favorites journal: removed leftover test favorite ${itemId}, which the server showed as set`,
  "not-favorite": (itemId) => `favorites journal: ${itemId} is not a favorite; dropped its entry`,
  missing: (itemId) => `favorites journal: ${itemId} no longer exists; dropped its entry`,
}

export async function reconcileFavorites(account, log) {
  const pending = readJournal().filter((entry) => entry.server === account.server && entry.userId === account.userId)
  if (!pending.length) {
    log(`favorites journal: nothing left over for ${account.userName} on ${account.server}`)
    return
  }
  const errors = []
  for (const { itemId } of pending) {
    try {
      log(RECONCILED[await settleFavorite(account, itemId)](itemId))
    } catch (error) {
      errors.push(`${itemId}: ${error.message}`)
    }
  }
  if (errors.length) throw new Error(`could not remove leftover test favorites listed in ${journalFile}: ${errors.join("; ")}`)
}
