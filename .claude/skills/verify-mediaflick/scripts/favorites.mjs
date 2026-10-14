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
  const entries = JSON.parse(readFileSync(journalFile, "utf8"))
  if (!Array.isArray(entries) || !entries.every((entry) => FIELDS.every((field) => typeof entry?.[field] === "string" && entry[field]))) {
    throw new Error(`${journalFile} is not a list of {server, userId, itemId}; check those favorites on the server, then fix or remove the file`)
  }
  return entries
}

function writeJournal(entries) {
  mkdirSync(path.dirname(journalFile), { recursive: true })
  const temporary = `${journalFile}.${process.pid}.tmp`
  writeFileSync(temporary, `${JSON.stringify(entries, null, 2)}\n`)
  renameSync(temporary, journalFile)
}

const isEntry = (account, itemId) => (entry) => entry.server === account.server && entry.userId === account.userId && entry.itemId === itemId

export function journalFavorite(account, itemId) {
  const entries = readJournal()
  if (!entries.some(isEntry(account, itemId))) writeJournal([...entries, { server: account.server, userId: account.userId, itemId }])
}

export async function isFavorite(account, itemId) {
  return (await jellyfinGet(account, `/Items/${encodeURIComponent(itemId)}?userId=${account.userId}`)).UserData.IsFavorite === true
}

// Returns whether the server still listed `itemId` as a favorite.
export async function settleFavorite(account, itemId) {
  const favorite = await isFavorite(account, itemId)
  if (favorite) await removeFavorite(account, itemId)
  writeJournal(readJournal().filter((entry) => !isEntry(account, itemId)(entry)))
  return favorite
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
      log((await settleFavorite(account, itemId)) ? `favorites journal: removed leftover test favorite ${itemId}` : `favorites journal: ${itemId} is not a favorite; dropped its entry`)
    } catch (error) {
      errors.push(`${itemId}: ${error.message}`)
    }
  }
  if (errors.length) throw new Error(`could not remove leftover test favorites listed in ${journalFile}: ${errors.join("; ")}`)
}
