import { execFileSync } from "node:child_process"
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs"
import path from "node:path"
import { parseArgs } from "node:util"

const root = path.resolve(import.meta.dirname, "../../../..")

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    run: { type: "string", default: "audit" },
    slices: { type: "string", default: "8" },
  },
})
const sliceCount = Number(values.slices)
if (!Number.isInteger(sliceCount) || sliceCount < 1) throw new Error(`--slices must be a positive integer, got ${values.slices}`)
if (!/^[\w.-]+$/.test(values.run)) throw new Error(`--run must be a plain name, got ${values.run}`)

const C_FAMILY = new Set([".rs", ".ts", ".tsx", ".js", ".mjs", ".cs", ".css"])
const HASH = new Set([".sh", ".ps1", ".py"])
const JS = new Set([".ts", ".tsx", ".js", ".mjs"])
const COMMENT_SUPPRESSION = /\b(oxlint-disable|eslint-disable|biome-ignore|prettier-ignore|noqa|pylint:\s*disable|type:\s*ignore|shellcheck\s+disable)\b|@ts-(ignore|expect-error|nocheck)/
const ATTRIBUTE_SUPPRESSION = {
  ".rs": /^\s*#!?\[(allow|expect)\(/,
  ".cs": /^\s*(#pragma\s+warning\s+disable|\[(assembly:\s*)?SuppressMessage)/,
}
const REGEX_PREFIX = new Set([..."(,=:[!&|?{};+-*%~^"])
const REGEX_KEYWORD = /\b(return|typeof|case|in|of|delete|void|throw|new|yield|await)$/

const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" })

function lineOf(src, index) {
  let line = 1
  for (let i = src.indexOf("\n"); i !== -1 && i < index; i = src.indexOf("\n", i + 1)) line++
  return line
}

function ownLine(src, index) {
  return /^[ \t]*$/.test(src.slice(src.lastIndexOf("\n", index - 1) + 1, index))
}

function lexCFamily(src, ext) {
  const comments = []
  const js = JS.has(ext)
  const templates = []
  let braceDepth = 0
  let last = ""
  let i = 0
  const push = (start, end, kind) => {
    comments.push({ index: start, endIndex: end, own: ownLine(src, start), kind, text: src.slice(start, end) })
  }
  const skipQuoted = (quote, multiline) => {
    let j = i + 1
    while (j < src.length && src[j] !== quote) {
      if (src[j] === "\n" && !multiline) return j
      j += src[j] === "\\" ? 2 : 1
    }
    return j + 1
  }
  while (i < src.length) {
    const c = src[i]
    const next = src[i + 1]
    if (c === "/" && next === "/" && ext !== ".css" && !(src[i - 1] === ":" && /\w/.test(src[i - 2] ?? ""))) {
      const end = src.indexOf("\n", i) === -1 ? src.length : src.indexOf("\n", i)
      const doc = (ext === ".rs" || ext === ".cs") && /^\/\/[/!](?!\/)/.test(src.slice(i, i + 4))
      push(i, end, doc ? "doc" : "line")
      i = end
      continue
    }
    if (c === "/" && next === "*") {
      const close = src.indexOf("*/", i + 2)
      const end = close === -1 ? src.length : close + 2
      const doc = /^\/\*[*!][^/]/.test(src.slice(i, i + 4))
      push(i, end, doc ? "doc" : "block")
      i = end
      continue
    }
    if (c === '"') {
      if (ext === ".rs") {
        const hashes = /r(#*)$/.exec(src.slice(Math.max(0, i - 64), i))
        if (hashes) {
          const close = src.indexOf(`"${hashes[1]}`, i + 1)
          i = close === -1 ? src.length : close + 1 + hashes[1].length
          last = '"'
          continue
        }
      }
      if (ext === ".cs" && src.startsWith('"""', i)) {
        const close = src.indexOf('"""', i + 3)
        i = close === -1 ? src.length : close + 3
        last = '"'
        continue
      }
      if (ext === ".cs" && /[@$]{1,2}$/.test(src.slice(Math.max(0, i - 2), i)) && src.slice(Math.max(0, i - 2), i).includes("@")) {
        let j = i + 1
        while (j < src.length && !(src[j] === '"' && src[j + 1] !== '"')) j += src[j] === '"' ? 2 : 1
        i = j + 1
        last = '"'
        continue
      }
      i = skipQuoted('"', !js)
      last = '"'
      continue
    }
    if (c === "'") {
      if (ext === ".rs") {
        const literal = /'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'/y
        literal.lastIndex = i
        i = literal.test(src) ? literal.lastIndex : i + 1
      } else {
        i = skipQuoted("'", false)
      }
      last = "'"
      continue
    }
    if (js && c === "`") {
      i = scanTemplate(i + 1)
      last = "`"
      continue
    }
    if (js && c === "/" && (last === "" || REGEX_PREFIX.has(last) || REGEX_KEYWORD.test(src.slice(Math.max(0, i - 12), i).trimEnd()))) {
      const end = scanRegex(i)
      if (end !== -1) {
        i = end
        last = "/"
        continue
      }
    }
    if (js && templates.length > 0) {
      if (c === "{") braceDepth++
      if (c === "}") {
        if (braceDepth === 0) {
          braceDepth = templates.pop()
          i = scanTemplate(i + 1)
          last = "`"
          continue
        }
        braceDepth--
      }
    }
    if (!/\s/.test(c)) last = c
    i++
  }
  return comments

  function scanTemplate(start) {
    let j = start
    while (j < src.length) {
      if (src[j] === "\\") j += 2
      else if (src[j] === "`") return j + 1
      else if (src[j] === "$" && src[j + 1] === "{") {
        templates.push(braceDepth)
        braceDepth = 0
        return j + 2
      } else j++
    }
    return j
  }

  function scanRegex(start) {
    let j = start + 1
    let inClass = false
    while (j < src.length && src[j] !== "\n") {
      if (src[j] === "\\") j += 2
      else if (src[j] === "[") inClass = true, j++
      else if (src[j] === "]") inClass = false, j++
      else if (src[j] === "/" && !inClass) {
        j++
        while (/[a-z]/.test(src[j] ?? "")) j++
        return j
      } else j++
    }
    return -1
  }
}

function lexHash(src, ext) {
  const comments = []
  const lines = src.split("\n")
  let offset = 0
  let state = null
  for (const [n, line] of lines.entries()) {
    let i = 0
    if (n === 0 && line.startsWith("#!")) i = line.length
    while (i < line.length) {
      if (state?.type === "heredoc") {
        if ((state.strip ? line.trimStart() : line) === state.word) state = state.resume
        i = line.length
        break
      }
      if (state?.type === "block") {
        const close = line.indexOf(state.close, i)
        state.text.push(line)
        if (close === -1) {
          i = line.length
          break
        }
        comments.push({ index: state.index, endIndex: offset + close + state.close.length, own: state.own, kind: state.kind, text: state.text.join("\n") })
        i = close + state.close.length
        state = null
        continue
      }
      if (state?.type === "string") {
        const close = line.indexOf(state.close, i)
        if (close === -1) {
          i = line.length
          break
        }
        i = close + state.close.length
        state = null
        continue
      }
      const rest = line.slice(i)
      const c = line[i]
      if (ext === ".ps1" && rest.startsWith("<#")) {
        state = { type: "block", close: "#>", index: offset + i, own: /^\s*$/.test(line.slice(0, i)), kind: "block", text: [] }
        continue
      }
      if (ext === ".ps1" && /^@["']\s*$/.test(rest)) {
        state = { type: "string", close: `${rest[1]}@` }
        i = line.length
        break
      }
      if (ext === ".py" && (rest.startsWith('"""') || rest.startsWith("'''"))) {
        const quote = rest.slice(0, 3)
        const atStatement = /^\s*[rRbBuUfF]*$/.test(line.slice(0, i))
        const close = line.indexOf(quote, i + 3)
        if (atStatement) {
          state = { type: "block", close: quote, index: offset + i, own: true, kind: "doc", text: [] }
          if (close !== -1) {
            comments.push({ index: offset + i, endIndex: offset + close + 3, own: true, kind: "doc", text: line.slice(i, close + 3) })
            state = null
            i = close + 3
          } else {
            state.text.push(line)
            i = line.length
          }
          continue
        }
        if (close === -1) {
          state = { type: "string", close: quote }
          i = line.length
        } else i = close + 3
        continue
      }
      if (c === "'" || c === '"') {
        let j = i + 1
        while (j < line.length && line[j] !== c) j += c === '"' && (line[j] === "\\" || line[j] === "`") ? 2 : 1
        if (j >= line.length && ext !== ".py") {
          state = { type: "string", close: c }
          i = line.length
        } else i = j + 1
        continue
      }
      if (c === "#" && (ext === ".py" || i === 0 || /\s/.test(line[i - 1]))) {
        comments.push({ index: offset + i, endIndex: offset + line.length, own: /^\s*$/.test(line.slice(0, i)), kind: "line", text: line.slice(i) })
        break
      }
      i++
    }
    const heredoc = ext === ".sh" && state?.type !== "heredoc" ? /<<(-?)\s*(['"]?)(\w+)\2/.exec(line) : null
    const commentAt = comments.at(-1)?.index ?? -1
    if (heredoc && !(commentAt >= offset && commentAt - offset < heredoc.index)) {
      state = { type: "heredoc", strip: heredoc[1] === "-", word: heredoc[3], resume: state?.type === "string" ? state : null }
    }
    offset += line.length + 1
  }
  return comments
}

function blocksFor(file) {
  const ext = path.extname(file)
  const src = readFileSync(path.join(root, file), "utf8")
  const raw = C_FAMILY.has(ext) ? lexCFamily(src, ext) : lexHash(src, ext)
  const blocks = []
  for (const comment of raw) {
    const start = lineOf(src, comment.index)
    const end = lineOf(src, comment.endIndex)
    const kind = COMMENT_SUPPRESSION.test(comment.text) ? "suppression" : comment.kind
    const previous = blocks.at(-1)
    const lineStyle = /^\s*(\/\/|#(?!>))/.test(comment.text)
    if (previous?.lineStyle && lineStyle && previous.own && comment.own && previous.kind === kind && previous.end === start - 1) {
      previous.end = end
      previous.text += `\n${comment.text.trim()}`
      continue
    }
    blocks.push({ path: file, start, end, kind, own: comment.own, lineStyle, text: comment.own ? comment.text.trim() : comment.text })
  }
  const attribute = ATTRIBUTE_SUPPRESSION[ext]
  if (attribute) {
    for (const [n, line] of src.split("\n").entries()) {
      if (attribute.test(line)) blocks.push({ path: file, start: n + 1, end: n + 1, kind: "suppression", own: true, text: line.trim() })
    }
  }
  blocks.sort((a, b) => a.start - b.start)
  const seen = new Map()
  return blocks.map(({ own: _own, lineStyle: _lineStyle, ...block }) => {
    const base = `${file}:${block.start}`
    const count = (seen.get(base) ?? 0) + 1
    seen.set(base, count)
    return { id: count === 1 ? base : `${base}#${count}`, ...block }
  })
}

const out = path.join(root, "tmp", "prune-comments", values.run)
if (existsSync(out) && readdirSync(out).some((name) => name.endsWith(".verdicts.json"))) {
  throw new Error(`${path.relative(root, out)} already holds verdicts; pick a new --run`)
}

const files = git("ls-files", "-z", "--", ...(positionals.length > 0 ? positionals : ["."]))
  .split("\0")
  .filter((file) => file && (C_FAMILY.has(path.extname(file)) || HASH.has(path.extname(file))))
const perFile = files.map((file) => ({ file, blocks: blocksFor(file) })).filter(({ blocks }) => blocks.length > 0)
const total = perFile.reduce((sum, { blocks }) => sum + blocks.length, 0)
if (total === 0) throw new Error("no comments in scope")

const slices = Array.from({ length: Math.min(sliceCount, perFile.length) }, () => ({ files: [], blocks: [] }))
let before = 0
for (const { file, blocks } of perFile) {
  const k = Math.min(slices.length - 1, Math.floor(((before + blocks.length / 2) / total) * slices.length))
  slices[k].files.push(file)
  slices[k].blocks.push(...blocks)
  before += blocks.length
}

const commit = git("rev-parse", "HEAD").trim()
const dirty = git("status", "--porcelain", "--", ...files).trim() !== ""
mkdirSync(out, { recursive: true })
const nonEmpty = slices.filter((slice) => slice.blocks.length > 0)
writeFileSync(
  path.join(out, "inventory.json"),
  `${JSON.stringify({ commit, dirty, scope: positionals, slices: nonEmpty.length, files: perFile.length, blocks: perFile.flatMap(({ blocks }) => blocks) }, null, 2)}\n`,
)
for (const [k, slice] of nonEmpty.entries()) {
  writeFileSync(path.join(out, `slice-${k + 1}.todo.json`), `${JSON.stringify({ slice: k + 1, commit, ...slice }, null, 2)}\n`)
}

console.log(`${path.relative(root, out)}  commit ${commit.slice(0, 12)}${dirty ? " (dirty)" : ""}  ${perFile.length} files  ${total} blocks`)
for (const [k, slice] of nonEmpty.entries()) {
  console.log(`slice-${k + 1}  ${String(slice.files.length).padStart(3)} files  ${String(slice.blocks.length).padStart(4)} blocks  ${slice.files[0]} .. ${slice.files.at(-1)}`)
}
