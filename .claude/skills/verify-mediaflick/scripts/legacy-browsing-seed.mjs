// Writes <dir>/accounts.json for `just verify library-browse --seed <dir>`: the
// cached account of SERVER remembers legacy Movie and Series pages, as a file
// from before per-library pages would, with Remember filters on. Holds the
// server and user ids only, never the token.
//   node .claude/skills/verify-mediaflick/scripts/legacy-browsing-seed.mjs https://jellyfin.example.com build/verify-seeds/legacy-browsing
import { mkdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { readAccount } from "./account.mjs"

const [server, dir] = process.argv.slice(2)
if (!server || !dir) throw new Error("usage: legacy-browsing-seed.mjs SERVER DIR")
const account = readAccount(server)
if (!account) throw new Error(`no cached session for ${server}; run \`just verify-login ${server}\` first`)
mkdirSync(dir, { recursive: true })
const file = path.join(dir, "accounts.json")
writeFileSync(file, `${JSON.stringify({
  version: 1,
  accounts: [{
    serverId: account.serverId,
    userId: account.userId,
    viewing: { rememberFilters: true },
    browsing: {
      last: "/library?kind=Movie&sort=year",
      Movie: "/library?kind=Movie&sort=year&filters=true",
      Series: "/library?kind=Series&watched=unwatched&filters=true",
    },
  }],
}, null, 2)}\n`)
console.log(`wrote ${file}`)
