// A real Jellyfin session for verify runs, without a password anywhere.
//
// `just verify-login SERVER` signs in once through Quick Connect, approved by
// the user in Jellyfin, and caches what the app stored: server, user, device
// id and access token. `just verify <drive> --account SERVER` seeds each
// disposable profile with that session, so the app starts signed in.
//
// The cache lives outside the repo, the build dir and the real MediaFlick
// profile, readable only by the user (0600 in a 0700 dir on Unix). The token
// never enters steps.log or evidence: the session scans evidence for it after
// every run. Revoke it in Jellyfin under Dashboard → Devices.

import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { DatabaseSync } from "node:sqlite"
import { isWindows } from "./platform.mjs"

const FIELDS = ["server", "serverUrl", "serverId", "userId", "userName", "deviceId", "token", "savedAt"]

export function accountsDir(env = process.env) {
  const root = isWindows
    ? env.APPDATA ?? path.join(env.USERPROFILE ?? os.homedir(), "AppData", "Roaming")
    : env.XDG_CONFIG_HOME || path.join(env.HOME ?? os.homedir(), ".config")
  return path.join(root, "mediaflick-verify", "accounts")
}

export function serverKey(server) {
  let url
  try {
    url = new URL(server.trim())
  } catch {
    throw new Error(`${JSON.stringify(server)} is not a server address; pass one like https://jellyfin.example.com`)
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error(`${server} must be an http or https address`)
  const normalized = `${url.protocol}//${url.host}${url.pathname.replace(/\/+$/, "")}`
  return { normalized, file: path.join(accountsDir(), `${normalized.replace(/^(https?):\/\//, "$1_").replace(/[^A-Za-z0-9.-]/g, "_")}.json`) }
}

export function readAccount(server) {
  const { normalized, file } = serverKey(server)
  if (!existsSync(file)) return null
  const account = JSON.parse(readFileSync(file, "utf8"))
  const missing = FIELDS.filter((field) => typeof account[field] !== "string" || !account[field])
  if (missing.length || account.server !== normalized) throw new Error(`${file} is not a cached session for ${normalized} (missing ${missing.join(", ") || "nothing"}); run \`just verify-login ${normalized}\` again`)
  return account
}

export function listAccounts() {
  const dir = accountsDir()
  if (!existsSync(dir)) return []
  return readdirSync(dir)
    .filter((name) => name.endsWith(".json"))
    .map((name) => {
      try {
        const { server, userName, savedAt } = JSON.parse(readFileSync(path.join(dir, name), "utf8"))
        return { server, userName, savedAt }
      } catch {
        return { server: name, userName: null, savedAt: null }
      }
    })
}

export function readStoredSession(dataDir) {
  const database = new DatabaseSync(path.join(dataDir, "library.db"), { readOnly: true })
  try {
    return database.prepare("SELECT server_url, server_id, user_id, user_name, device_id, token FROM credentials WHERE id = 1").get() ?? null
  } finally {
    database.close()
  }
}

export function saveAccount(server, dataDir) {
  const { normalized, file } = serverKey(server)
  const stored = readStoredSession(dataDir)
  const account = {
    server: normalized,
    serverUrl: stored?.server_url,
    serverId: stored?.server_id,
    userId: stored?.user_id,
    userName: stored?.user_name,
    deviceId: stored?.device_id,
    token: stored?.token,
    savedAt: new Date().toISOString(),
  }
  const missing = FIELDS.filter((field) => typeof account[field] !== "string" || !account[field])
  if (missing.length) throw new Error(`the app has no complete signed-in session to cache (missing ${missing.join(", ")})`)
  const dir = path.dirname(file)
  mkdirSync(dir, { recursive: true, mode: 0o700 })
  if (!isWindows) chmodSync(dir, 0o700)
  const temporary = `${file}.${process.pid}.tmp`
  writeFileSync(temporary, `${JSON.stringify(account, null, 2)}\n`, { mode: 0o600 })
  renameSync(temporary, file)
  return { account, file }
}

// Writes a library.db holding only the session, stamped with an old schema
// version. The app's pre-1.0 recreation path (src/library/db.rs `migrate`)
// then builds the current schema and keeps the session, the same way an
// upgrade keeps a user signed in.
export function seedLibrary(dataDir, account) {
  const file = path.join(dataDir, "library.db")
  if (existsSync(file)) throw new Error(`${file} already exists; refusing to seed over it`)
  const database = new DatabaseSync(file)
  try {
    database.exec(`CREATE TABLE credentials (
      id INTEGER PRIMARY KEY CHECK (id = 1), server_url TEXT, user_id TEXT, user_name TEXT,
      server_id TEXT, device_id TEXT NOT NULL, token TEXT, updated_at INTEGER NOT NULL
    )`)
    database
      .prepare("INSERT INTO credentials (id, server_url, user_id, user_name, server_id, device_id, token, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?, ?)")
      .run(account.serverUrl, account.userId, account.userName, account.serverId, account.deviceId, account.token, Math.floor(Date.now() / 1000))
    database.exec("PRAGMA user_version = 1")
  } finally {
    database.close()
  }
}

// Asks the server whether the cached token still works. A header with only
// the token makes Jellyfin take client, device and device id from the token's
// own device record, so the check changes nothing on the server.
export async function checkAccount(account) {
  let response
  try {
    response = await fetch(`${account.serverUrl}/Users/Me`, {
      headers: { Authorization: `MediaBrowser Token="${account.token}"`, Accept: "application/json" },
      signal: AbortSignal.timeout(15000),
    })
  } catch (error) {
    throw new Error(`cannot reach ${account.serverUrl} to check the cached session: ${error.cause?.message ?? error.message}`)
  }
  if (response.status === 401 || response.status === 403) {
    throw new Error(`${account.server} rejected the cached session for ${account.userName} (HTTP ${response.status}); it was revoked or signed out. Run \`just verify-login ${account.server}\` again`)
  }
  if (!response.ok) throw new Error(`${account.serverUrl}/Users/Me answered HTTP ${response.status} for the cached session`)
  const user = await response.json()
  if (user?.Id?.replace(/-/g, "").toLowerCase() !== account.userId.replace(/-/g, "").toLowerCase()) {
    throw new Error(`the cached token for ${account.server} belongs to ${user?.Name ?? "another user"}, not ${account.userName}`)
  }
  return { status: response.status, userName: user.Name }
}

export async function jellyfinGet(account, apiPath) {
  const response = await fetch(`${account.serverUrl}${apiPath}`, {
    headers: { Authorization: `MediaBrowser Token="${account.token}"`, Accept: "application/json" },
    signal: AbortSignal.timeout(20000),
  })
  if (!response.ok) throw Object.assign(new Error(`GET ${apiPath} on ${account.server} answered HTTP ${response.status}`), { status: response.status })
  return response.json()
}

export async function resetUserData(account, itemId, { played = false, positionTicks = 0 } = {}) {
  const response = await fetch(`${account.serverUrl}/UserItems/${encodeURIComponent(itemId)}/UserData?userId=${account.userId}`, {
    method: "POST",
    headers: { Authorization: `MediaBrowser Token="${account.token}"`, "Content-Type": "application/json" },
    body: JSON.stringify({ PlaybackPositionTicks: positionTicks, Played: played }),
    signal: AbortSignal.timeout(20000),
  })
  if (!response.ok) throw new Error(`resetting user data of ${itemId} answered HTTP ${response.status}`)
}

export async function playlistWrite(account, method, apiPath, body) {
  if (!/^\/(Playlists|Items\/[0-9a-f]{32}$)/.test(apiPath)) throw new Error(`playlistWrite only writes playlists, not ${apiPath}`)
  const response = await fetch(`${account.serverUrl}${apiPath}`, {
    method,
    headers: { Authorization: `MediaBrowser Token="${account.token}"`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(20000),
  })
  if (!response.ok) throw new Error(`${method} ${apiPath} answered HTTP ${response.status}`)
  const text = await response.text()
  return text ? JSON.parse(text) : null
}

export async function removeFavorite(account, itemId) {
  const response = await fetch(`${account.serverUrl}/UserFavoriteItems/${encodeURIComponent(itemId)}?userId=${account.userId}`, {
    method: "DELETE",
    headers: { Authorization: `MediaBrowser Token="${account.token}"` },
    signal: AbortSignal.timeout(20000),
  })
  if (!response.ok) throw new Error(`removing ${itemId} from favorites answered HTTP ${response.status}`)
}

export async function cancelSeerrRequest(account, requestId) {
  const response = await fetch(`${account.serverUrl}/MediaFlick/seerr/request/${Number(requestId)}`, {
    method: "DELETE",
    headers: { Authorization: `MediaBrowser Token="${account.token}"` },
    signal: AbortSignal.timeout(20000),
  })
  if (!response.ok) throw new Error(`cancelling Seerr request ${requestId} answered HTTP ${response.status}`)
}

export async function tokenStatus(serverUrl, token) {
  const response = await fetch(`${serverUrl}/Users/Me`, { headers: { Authorization: `MediaBrowser Token="${token}"` }, signal: AbortSignal.timeout(20000) })
  return response.status
}

export async function revokeToken(serverUrl, token) {
  const response = await fetch(`${serverUrl}/Sessions/Logout`, { method: "POST", headers: { Authorization: `MediaBrowser Token="${token}"` }, signal: AbortSignal.timeout(20000) })
  if (!response.ok) throw new Error(`revoking a token answered HTTP ${response.status}`)
}

export async function approveQuickConnect(account, code) {
  const response = await fetch(`${account.serverUrl}/QuickConnect/Authorize?code=${encodeURIComponent(code)}`, {
    method: "POST",
    headers: { Authorization: `MediaBrowser Token="${account.token}"` },
    signal: AbortSignal.timeout(20000),
  })
  if (!response.ok) throw new Error(`approving Quick Connect code ${code} answered HTTP ${response.status}`)
}

export function publicAccount(account) {
  const { token: _token, ...rest } = account
  return rest
}

// Removes every occurrence of `secret` from files under `dir` and returns the
// files that held it. latin1 maps bytes one to one, so binary files survive.
export function redactSecret(dir, secret) {
  const leaked = []
  const visit = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const file = path.join(current, entry.name)
      if (entry.isDirectory()) visit(file)
      else if (entry.isFile()) {
        const content = readFileSync(file)
        if (content.indexOf(secret) === -1) continue
        writeFileSync(file, Buffer.from(content.toString("latin1").split(secret).join("<redacted>"), "latin1"))
        leaked.push(file)
      }
    }
  }
  if (secret && existsSync(dir)) visit(dir)
  return leaked
}
