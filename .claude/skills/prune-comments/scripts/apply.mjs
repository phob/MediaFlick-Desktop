import { readFileSync, writeFileSync } from "node:fs"
import path from "node:path"
import { parseArgs } from "node:util"

const root = path.resolve(import.meta.dirname, "../../../..")
const { values } = parseArgs({
  options: {
    run: { type: "string", default: "audit" },
    "max-lines": { type: "string" },
    "min-lines": { type: "string", default: "1" },
    path: { type: "string", multiple: true, default: [] },
  },
})
const dir = path.join(root, "tmp", "prune-comments", values.run)
const report = JSON.parse(readFileSync(path.join(dir, "report.json"), "utf8"))
const inventory = JSON.parse(readFileSync(path.join(dir, "inventory.json"), "utf8"))
if (!report.complete) throw new Error("report.json is incomplete; run merge.mjs until it exits 0")

const minLines = Number(values["min-lines"])
const maxLines = values["max-lines"] === undefined ? Infinity : Number(values["max-lines"])
const byId = new Map(inventory.blocks.map((block) => [block.id, block]))
const blocks = report.verdicts
  .filter((entry) => entry.verdict === "DELETE")
  .map((entry) => byId.get(entry.id))
  .filter((block) => {
    const lines = block.end - block.start + 1
    return lines >= minLines && lines <= maxLines && (values.path.length === 0 || values.path.some((prefix) => block.path.startsWith(prefix)))
  })

const byFile = Map.groupBy(blocks, (block) => block.path)
const isBlank = (line) => line !== undefined && line.trim() === ""
const normalize = (text) => text.split("\n").map((line) => line.trim()).join("\n")
let applied = 0
const stale = []

for (const [file, fileBlocks] of byFile) {
  const absolute = path.join(root, file)
  const src = readFileSync(absolute, "utf8")
  const finalNewline = src.endsWith("\n")
  const lines = (finalNewline ? src.slice(0, -1) : src).split("\n")
  for (const block of fileBlocks.toSorted((a, b) => b.start - a.start || b.id.localeCompare(a.id))) {
    const span = lines.slice(block.start - 1, block.end)
    if (normalize(span.join("\n")) === normalize(block.text)) {
      lines.splice(block.start - 1, span.length)
      const at = block.start - 1
      if (isBlank(lines[at]) && (at === 0 || isBlank(lines[at - 1]) || lines[at - 1].trimEnd().endsWith("{"))) lines.splice(at, 1)
      else if (at === lines.length && isBlank(lines[at - 1])) lines.splice(at - 1, 1)
      applied++
      continue
    }
    const joined = span.join("\n")
    const index = joined.lastIndexOf(block.text)
    if (index === -1) {
      stale.push(block.id)
      continue
    }
    let before = joined.slice(0, index)
    let after = joined.slice(index + block.text.length)
    if (/\{\s*$/.test(before) && /^\s*\}/.test(after)) {
      before = before.replace(/\{\s*$/, "")
      after = after.replace(/^\s*\}/, "")
    }
    const indent = /^[ \t]*/.exec(span[0])[0]
    const rest = before.trim() === "" ? `${indent}${after.trimStart()}` : `${before.trimEnd()}${after}`
    const replacement = rest.split("\n").filter((line) => line.trim() !== "").map((line) => line.trimEnd())
    lines.splice(block.start - 1, span.length, ...replacement)
    applied++
  }
  writeFileSync(absolute, `${lines.join("\n")}${finalNewline ? "\n" : ""}`)
}

console.log(`deleted ${applied} blocks in ${byFile.size} files${stale.length > 0 ? `, ${stale.length} stale` : ""}`)
for (const id of stale) console.log(`stale: ${id}`)
process.exit(stale.length === 0 ? 0 : 1)
