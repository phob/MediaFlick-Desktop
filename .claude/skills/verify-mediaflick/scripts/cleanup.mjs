// Removes what interrupted verify runs left behind: processes whose command
// line names a dead run's disposable profile, then the profile itself. Live
// runs (their session.mjs still running, in any worktree) are skipped.
// Evidence under build/verify is never touched.
//
//   just verify-cleanup

import { rmSync } from "node:fs"
import { killPids, listProcesses } from "./platform.mjs"
import { scanRuns } from "./runs.mjs"

const runs = scanRuns()
if (!runs.length) console.log("nothing to clean")
for (const run of runs) {
  if (run.live) {
    console.log(`skip live run ${run.runId} (session pid ${run.sessionPid}): ${run.dir}`)
    continue
  }
  if (run.processes.length) {
    console.log(`stopping ${run.processes.map(({ pid, name }) => `${name} ${pid}`).join(", ")}`)
    killPids(run.processes.map(({ pid }) => pid))
  }
  // Re-check by command line; a stopped tree takes a moment to unwind.
  for (let attempt = 0; attempt < 20 && listProcesses().some((row) => row.cmd.includes(run.dir)); attempt++) await new Promise((resolve) => setTimeout(resolve, 500))
  try {
    rmSync(run.dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 })
    console.log(`removed ${run.dir}`)
  } catch (error) {
    console.error(`could not remove ${run.dir}: ${error.message}`)
    process.exitCode = 1
  }
}
