import { execFileSync } from "node:child_process"
import path from "node:path"
import { parseArgs } from "node:util"

const root = path.resolve(import.meta.dirname, "../../../..")
const { values } = parseArgs({ options: { base: { type: "string", default: "HEAD" } } })

const OPENER = /^(\{\s*)?(\/\/|\/\*|#|<#|"""|''')/
const diff = execFileSync("git", ["diff", "-U0", "--no-color", "--no-ext-diff", values.base, "--", ".", ":!.claude/skills/prune-comments"], {
  cwd: root,
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
})

const failures = []
let file = ""
let removed = []
let added = []
let hunks = 0

function closer(line) {
  if (/^\s*\{?\s*\/\*/.test(line) && !line.includes("*/")) return "*/"
  if (/^\s*<#/.test(line) && !line.includes("#>")) return "#>"
  const docstring = /^\s*[rRbBuUfF]*("""|''')/.exec(line)
  if (docstring && !line.slice(line.indexOf(docstring[1]) + 3).includes(docstring[1])) return docstring[1]
  return null
}

function truncation(minus, plus) {
  for (let k = 0; k < minus.length; k++) {
    const tail = minus.slice(k).trimStart()
    if (!OPENER.test(tail)) continue
    const head = minus.slice(0, k).trimEnd()
    if (head === plus.trimEnd()) return true
    const jsx = /^\{\s*\/\*[\s\S]*?\*\/\s*\}/.exec(tail)
    if (jsx && `${head}${minus.slice(k).trimStart().slice(jsx[0].length)}`.trimEnd() === plus.trimEnd()) return true
    if (jsx && `${head} ${minus.slice(k).trimStart().slice(jsx[0].length).trimStart()}`.trimEnd() === plus.trimEnd()) return true
  }
  return false
}

function flush() {
  if (removed.length === 0 && added.length === 0) return
  hunks++
  const used = new Set()
  for (const plus of added) {
    const match = removed.findIndex((minus, i) => !used.has(i) && truncation(minus, plus))
    if (match === -1) failures.push(`${file}: added line is not a removed line minus a comment: ${plus.trim()}`)
    else used.add(match)
  }
  let open = null
  for (const [i, minus] of removed.entries()) {
    if (used.has(i)) continue
    if (open) {
      if (minus.includes(open)) open = null
      continue
    }
    if (minus.trim() === "") continue
    if (!OPENER.test(minus.trimStart()) && !/^\s*\*/.test(minus)) {
      failures.push(`${file}: removed line is not a comment: ${minus.trim()}`)
      continue
    }
    open = closer(minus)
  }
  removed = []
  added = []
}

for (const line of diff.split("\n")) {
  if (line.startsWith("diff --git")) {
    flush()
    file = line.split(" b/")[1]
  } else if (line.startsWith("@@")) flush()
  else if (line.startsWith("-") && !line.startsWith("---")) removed.push(line.slice(1))
  else if (line.startsWith("+") && !line.startsWith("+++")) added.push(line.slice(1))
}
flush()

for (const failure of failures) console.log(failure)
console.log(failures.length === 0 ? `comment-only diff: ${hunks} hunks checked` : `${failures.length} non-comment changes`)
process.exit(failures.length === 0 ? 0 : 1)
