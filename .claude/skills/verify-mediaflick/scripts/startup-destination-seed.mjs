// Writes three seeds for `just verify startup-library --seed <dir>/<case>`, each
// an accounts.json whose account starts on one stored destination: `movies`
// (legacy), `library` (the server's first TV shows library) and `missing` (a
// well-formed library id the server does not list). SERVER is a server with a
// session cached by `just verify-login`, or the demo server. Holds ids only,
// never a token.
//   node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://demo.jellyfin.org/stable build/verify-seeds/startup-demo
//   node .claude/skills/verify-mediaflick/scripts/startup-destination-seed.mjs https://jellyfin.example.com build/verify-seeds/startup-account
import { mkdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { readAccount } from "./account.mjs"

const DEMO = "https://demo.jellyfin.org/stable"
const MISSING = "0123456789abcdef0123456789abcdef"
const CLIENT = 'MediaBrowser Client="MediaFlick verify seed", Device="verify", DeviceId="mediaflick-verify-startup-seed", Version="1"'

const [server, dir] = process.argv.slice(2)
if (!server || !dir) throw new Error("usage: startup-destination-seed.mjs SERVER DIR")

async function jellyfin(base, route, { token, method = "GET", body } = {}) {
  const response = await fetch(`${base}${route}`, {
    method,
    headers: { Authorization: token ? `${CLIENT}, Token="${token}"` : CLIENT, "Content-Type": "application/json" },
    body: body && JSON.stringify(body),
  })
  if (!response.ok) throw new Error(`${method} ${route} answered ${response.status}`)
  return response.status === 204 ? null : response.json().catch(() => null)
}

async function session() {
  const cached = readAccount(server)
  if (cached) return { base: cached.serverUrl.replace(/\/+$/, ""), serverId: cached.serverId, userId: cached.userId, token: cached.token, signOut: async () => {} }
  if (server.replace(/\/+$/, "") !== DEMO) throw new Error(`no cached session for ${server}; run \`just verify-login ${server}\` first`)
  const signedIn = await jellyfin(DEMO, "/Users/AuthenticateByName", { method: "POST", body: { Username: "demo", Pw: "" } })
  return {
    base: DEMO,
    serverId: signedIn.ServerId,
    userId: signedIn.User.Id,
    token: signedIn.AccessToken,
    signOut: () => jellyfin(DEMO, "/Sessions/Logout", { method: "POST", token: signedIn.AccessToken }),
  }
}

const account = await session()
let views
try {
  views = (await jellyfin(account.base, `/UserViews?userId=${account.userId}`, { token: account.token })).Items
} finally {
  await account.signOut()
}
const shows = views.find((view) => view.CollectionType === "tvshows")
if (!shows) throw new Error(`${server} lists no TV shows library for this user`)
if (!views.some((view) => view.CollectionType === "movies")) throw new Error(`${server} lists no movies library for this user`)
if (views.some((view) => view.Id.toLowerCase() === MISSING)) throw new Error(`${server} really has a library ${MISSING}`)

const destinations = { movies: "movies", library: { library: shows.Id.toLowerCase() }, missing: { library: MISSING } }
for (const [name, startupDestination] of Object.entries(destinations)) {
  const target = path.join(dir, name)
  mkdirSync(target, { recursive: true })
  writeFileSync(path.join(target, "accounts.json"), `${JSON.stringify({
    version: 1,
    accounts: [{ serverId: account.serverId, userId: account.userId, viewing: { startupDestination } }],
  }, null, 2)}\n`)
  console.log(`wrote ${path.join(target, "accounts.json")} (${JSON.stringify(startupDestination)})`)
}
