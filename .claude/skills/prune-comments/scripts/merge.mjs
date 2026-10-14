import { existsSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"
import { parseArgs } from "node:util"

const root = path.resolve(import.meta.dirname, "../../../..")
const { values } = parseArgs({ options: { run: { type: "string", default: "audit" }, slice: { type: "string" } } })
const dir = path.join(root, "tmp", "prune-comments", values.run)
const read = (name) => JSON.parse(readFileSync(path.join(dir, name), "utf8"))

const FIXES = new Set(["rename", "extract", "type", "test", "delete-dead-path", "use-real-api", "rearchitect"])
const EXCEPTIONS = new Set(["license", "foreign-constraint", "lint-required", "lint-rule-faulty", "api-contract", "issue-link", "prettier-ignore"])
const FIELDS = {
  DELETE: ["id", "verdict", "why"],
  REFACTOR: ["id", "verdict", "why", "symbol", "fix"],
  KEEP: ["id", "verdict", "why", "exception"],
  SKIP: ["id", "verdict", "why"],
}

const inventory = read("inventory.json")
const byId = new Map(inventory.blocks.map((block) => [block.id, block]))
const errors = []
const gaps = {}
const verdicts = []

const only = values.slice === undefined ? null : Number(values.slice)
if (only !== null && !(Number.isInteger(only) && only >= 1 && only <= inventory.slices)) throw new Error(`--slice must be 1..${inventory.slices}`)

for (let k = only ?? 1; k <= (only ?? inventory.slices); k++) {
  const todo = read(`slice-${k}.todo.json`)
  const expected = new Set(todo.blocks.map((block) => block.id))
  const name = `slice-${k}.verdicts.json`
  if (!existsSync(path.join(dir, name))) {
    gaps[k] = [...expected]
    continue
  }
  const result = read(name)
  if (result.commit !== inventory.commit) errors.push(`${name}: commit ${result.commit} is not ${inventory.commit}`)
  if (!Array.isArray(result.verdicts)) {
    errors.push(`${name}: verdicts is not an array`)
    gaps[k] = [...expected]
    continue
  }
  const seen = new Set()
  for (const entry of result.verdicts) {
    const where = `${name} ${entry?.id ?? "<no id>"}`
    const fields = FIELDS[entry?.verdict]
    if (!fields) {
      errors.push(`${where}: verdict must be one of ${Object.keys(FIELDS).join(", ")}`)
      continue
    }
    const extra = Object.keys(entry).filter((key) => !fields.includes(key))
    const empty = fields.filter((key) => typeof entry[key] !== "string" || entry[key].trim() === "")
    if (extra.length > 0) errors.push(`${where}: unexpected ${extra.join(", ")}`)
    if (empty.length > 0) errors.push(`${where}: missing ${empty.join(", ")}`)
    if (entry.verdict === "REFACTOR" && !FIXES.has(entry.fix)) errors.push(`${where}: fix must be one of ${[...FIXES].join(", ")}`)
    if (entry.verdict === "KEEP" && !EXCEPTIONS.has(entry.exception)) errors.push(`${where}: exception must be one of ${[...EXCEPTIONS].join(", ")}`)
    if (!expected.has(entry.id)) errors.push(`${where}: not a block of slice ${k}`)
    else if (seen.has(entry.id)) errors.push(`${where}: duplicate verdict`)
    seen.add(entry.id)
    verdicts.push(entry)
  }
  const missing = [...expected].filter((id) => !seen.has(id))
  if (missing.length > 0) gaps[k] = missing
}

if (only !== null) {
  for (const error of errors) console.log(`error: ${error}`)
  for (const id of gaps[only] ?? []) console.log(`gap: ${id}`)
  console.log(errors.length === 0 && !gaps[only] ? `slice ${only} clean` : `slice ${only} has ${errors.length} errors, ${gaps[only]?.length ?? 0} gaps`)
  process.exit(errors.length === 0 && !gaps[only] ? 0 : 1)
}

const LONG_DELETE = 4
const area = (file) => file.split("/").slice(0, file.startsWith("src/") || file.startsWith("ui/") ? 3 : 2).join("/")
const lines = (entry) => {
  const block = byId.get(entry.id)
  return block ? block.end - block.start + 1 : 0
}
const totals = {}
const areas = {}
for (const entry of verdicts) {
  const block = byId.get(entry.id)
  if (!block) continue
  totals[entry.verdict] = (totals[entry.verdict] ?? 0) + 1
  const row = (areas[area(block.path)] ??= { DELETE: 0, REFACTOR: 0, KEEP: 0, SKIP: 0, longDeletes: 0, deletedLines: 0 })
  row[entry.verdict]++
  if (entry.verdict === "DELETE" && lines(entry) >= LONG_DELETE) row.longDeletes++
  if (entry.verdict === "DELETE" || entry.verdict === "REFACTOR") row.deletedLines += lines(entry)
}
const pick = (verdict) => verdicts.filter((entry) => entry.verdict === verdict && byId.has(entry.id))
const complete = errors.length === 0 && Object.keys(gaps).length === 0

writeFileSync(
  path.join(dir, "report.json"),
  `${JSON.stringify({ commit: inventory.commit, complete, totals, areas, errors, gaps, verdicts }, null, 2)}\n`,
)

const md = [
  `# Comment audit ${values.run}`,
  "",
  `Commit \`${inventory.commit}\`${inventory.dirty ? " (dirty tree)" : ""}. ${inventory.files} files, ${inventory.blocks.length} blocks. ${complete ? "Complete." : "INCOMPLETE: see errors and gaps."}`,
  "",
  `Deletes of ${LONG_DELETE} or more lines are counted again under Long, because rationale hides in them.`,
  "",
  "| Area | Delete | Long | Refactor | Keep | Skip | Comment lines removed |",
  "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
  ...Object.entries(areas)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, row]) => `| \`${name}\` | ${row.DELETE} | ${row.longDeletes} | ${row.REFACTOR} | ${row.KEEP} | ${row.SKIP} | ${row.deletedLines} |`),
  `| **Total** | ${totals.DELETE ?? 0} | ${Object.values(areas).reduce((sum, row) => sum + row.longDeletes, 0)} | ${totals.REFACTOR ?? 0} | ${totals.KEEP ?? 0} | ${totals.SKIP ?? 0} | ${Object.values(areas).reduce((sum, row) => sum + row.deletedLines, 0)} |`,
  "",
  "## Refactor",
  "",
  ...pick("REFACTOR").map((entry) => `- \`${entry.id}\` ${entry.fix} \`${entry.symbol}\`. ${entry.why}`),
  "",
  "## Keep",
  "",
  ...pick("KEEP").map((entry) => `- \`${entry.id}\` ${entry.exception}. ${entry.why}`),
  "",
  ...(errors.length > 0 ? ["## Errors", "", ...errors.map((error) => `- ${error}`), ""] : []),
  ...(Object.keys(gaps).length > 0 ? ["## Gaps", "", ...Object.entries(gaps).map(([k, ids]) => `- slice ${k}: ${ids.length} blocks without a verdict`), ""] : []),
].join("\n")
writeFileSync(path.join(dir, "report.md"), md)

console.log(`${path.relative(root, dir)}/report.md  ${JSON.stringify(totals)}`)
for (const error of errors.slice(0, 20)) console.log(`error: ${error}`)
for (const [k, ids] of Object.entries(gaps)) console.log(`gap: slice ${k} has ${ids.length} blocks without a verdict`)
process.exit(complete ? 0 : 1)
