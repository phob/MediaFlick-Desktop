// Fails on any comment in src/ that silences a lint or type check. An oxlint
// rule could not enforce this: `/* oxlint-disable */` silences that rule too.
// A rule that misfires on a whole file is turned off for that file in
// `.oxlintrc.json` overrides, where the exception is reviewed once.

import { readdirSync, readFileSync } from "node:fs"
import { join } from "node:path"

const SUPPRESSION = /(?:oxlint|eslint)-disable|@ts-(?:ignore|expect-error|nocheck)/
const SOURCE = /\.(?:[cm]?[jt]sx?)$/

const found = readdirSync("src", { recursive: true, encoding: "utf8" })
  .filter((path) => SOURCE.test(path))
  .flatMap((path) =>
    readFileSync(join("src", path), "utf8")
      .split("\n")
      .flatMap((line, index) => {
        const match = SUPPRESSION.exec(line)
        return match ? [`src/${path}:${index + 1}: \`${match[0]}\` suppresses a check`] : []
      }),
  )

for (const line of found) console.error(line)
if (found.length > 0) {
  console.error("Fix the code, or turn a misfiring rule off for its file in .oxlintrc.json overrides.")
  process.exit(1)
}
