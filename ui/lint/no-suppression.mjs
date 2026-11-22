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
