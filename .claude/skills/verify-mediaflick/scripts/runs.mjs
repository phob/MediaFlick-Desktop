// Disposable profiles of verify runs, live or left behind. Shared by the
// doctor and cleanup. A profile is named
//   <tmp>/mediaflick-verify-<run-id>-<session-pid>-<random>
// so a run's liveness is a question about one PID, even across worktrees.

import { readdirSync, statSync } from "node:fs"
import path from "node:path"
import { PROFILE_PREFIX, isAppProcess, listProcesses, profileParent } from "./platform.mjs"

const NAME = new RegExp(`^${PROFILE_PREFIX}(.+)-(\\d+)-([0-9a-f]{6})$`)

export const profileName = (runId, sessionPid, random) => `${PROFILE_PREFIX}${runId}-${sessionPid}-${random}`

export function scanRuns(processes = listProcesses()) {
  const sessions = new Map(processes.filter((row) => /session\.mjs/.test(row.cmd) && /node/i.test(row.name)).map((row) => [row.pid, row]))
  const runs = []
  for (const entry of readdirSync(profileParent)) {
    if (!entry.startsWith(PROFILE_PREFIX)) continue
    const dir = path.join(profileParent, entry)
    try {
      if (!statSync(dir).isDirectory()) continue
    } catch {
      continue
    }
    const match = NAME.exec(entry)
    const sessionPid = match ? Number(match[2]) : null
    runs.push({
      dir,
      runId: match?.[1] ?? null,
      sessionPid,
      live: sessionPid !== null && sessions.has(sessionPid),
      processes: processes.filter((row) => row.cmd.includes(dir)).map(({ pid, name }) => ({ pid, name })),
    })
  }
  return runs
}

// The user's own MediaFlick sessions: app browser processes not started by a verify run.
export const userApps = (processes = listProcesses()) =>
  processes.filter((row) => isAppProcess(row) && !row.cmd.includes(PROFILE_PREFIX) && !/--type=/.test(row.cmd))
