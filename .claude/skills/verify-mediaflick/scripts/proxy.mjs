// A loopback Jellyfin proxy for drives that need a server behaving in a
// controlled way. It forwards every request to `upstream` (including its path
// prefix, such as `/stable`), streaming bodies both ways with their headers
// (so ranged video requests work) and passing WebSocket upgrades (the app's
// `/socket`) through. Two hooks change what the app sees:
// - `fail(url, method)` drops the connection of each request it selects, which
//   the app sees as an unreachable server; `heal()` stops failing.
// - `answer(url, method)` returns `{ status, body }` to answer a request
//   itself with JSON instead of forwarding it, or null to forward it.
// `cutSockets()` drops every open WebSocket and refuses new upgrades until
// `restoreSockets()`, as a lost network would.
// The app accepts `http://127.0.0.1:<port>` as a server address, so a drive
// signs in to `proxy.url`.

import http from "node:http"
import https from "node:https"

export function startProxy({ upstream, fail = () => false, answer = () => null, log }) {
  const base = new URL(upstream)
  const prefix = base.pathname.replace(/\/+$/, "")
  const transport = base.protocol === "https:" ? https : http
  const stats = { forwarded: 0, failed: 0, answered: [], streams: [], upgrades: 0, refusedUpgrades: 0 }
  const upgraded = new Set()
  let failing = true
  let socketsCut = false

  const target = (request, url) => ({
    protocol: base.protocol,
    hostname: base.hostname,
    port: base.port || undefined,
    method: request.method,
    path: `${prefix}${url.pathname}${url.search}`,
    headers: { ...request.headers, host: base.host },
  })

  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://proxy.invalid")
    const method = request.method ?? "GET"
    if (failing && fail(url, method)) {
      stats.failed++
      if (stats.failed === 1) log(`proxy: failing ${method} ${url.pathname} (first failure)`)
      request.socket.destroy()
      return
    }
    const canned = answer(url, method)
    if (canned) {
      stats.answered.push({ at: new Date().toISOString(), method, path: url.pathname, search: url.search, status: canned.status })
      log(`proxy: answered ${method} ${url.pathname} with HTTP ${canned.status}`)
      const body = JSON.stringify(canned.body)
      response.writeHead(canned.status, { "content-type": "application/json; charset=utf-8", "content-length": Buffer.byteLength(body) })
      response.end(body)
      request.resume()
      return
    }
    stats.forwarded++
    const stream = /^\/(Videos|Audio)\//i.test(url.pathname) ? { at: new Date().toISOString(), path: url.pathname, range: request.headers.range ?? null, status: null, bytes: 0, firstByteMs: null, durationMs: null, end: null } : null
    const started = Date.now()
    if (stream) {
      stats.streams.push(stream)
      log(`proxy: stream ${method} ${url.pathname} range=${stream.range ?? "none"}`)
    }
    const outbound = transport.request(target(request, url), (reply) => {
      if (stream) {
        stream.status = reply.statusCode
        stream.firstByteMs = Date.now() - started
        reply.on("data", (chunk) => { stream.bytes += chunk.length })
      }
      response.writeHead(reply.statusCode ?? 502, reply.headers)
      reply.pipe(response)
    })
    // mpv drops a ranged stream on every seek; stop downloading it upstream.
    response.on("close", () => {
      const finished = response.writableFinished
      if (!finished) outbound.destroy()
      if (stream) {
        stream.durationMs = Date.now() - started
        stream.end = finished ? "complete" : "client closed"
        log(`proxy: stream ${url.pathname} range=${stream.range ?? "none"} → HTTP ${stream.status ?? "none"} after ${stream.firstByteMs ?? "-"} ms, ${stream.end} after ${stream.bytes} bytes in ${stream.durationMs} ms`)
      }
    })
    outbound.on("error", (error) => {
      if (response.destroyed) return
      log(`proxy: upstream ${method} ${url.pathname} failed: ${error.message}`)
      if (!response.headersSent) response.writeHead(502)
      response.end()
    })
    request.pipe(outbound)
  })

  server.on("upgrade", (request, socket, head) => {
    const url = new URL(request.url ?? "/", "http://proxy.invalid")
    if (socketsCut) {
      stats.refusedUpgrades++
      log(`proxy: refused the upgrade of ${url.pathname} while sockets are cut`)
      socket.destroy()
      return
    }
    upgraded.add(socket)
    socket.on("close", () => upgraded.delete(socket))
    socket.on("error", () => socket.destroy())
    const outbound = transport.request(target(request, url))
    const writeHead = (reply, skip = [], extra = []) => {
      const lines = [`HTTP/1.1 ${reply.statusCode} ${reply.statusMessage}`]
      for (let i = 0; i < reply.rawHeaders.length; i += 2) {
        if (!skip.includes(reply.rawHeaders[i].toLowerCase())) lines.push(`${reply.rawHeaders[i]}: ${reply.rawHeaders[i + 1]}`)
      }
      socket.write(`${[...lines, ...extra].join("\r\n")}\r\n\r\n`)
    }
    outbound.on("upgrade", (reply, upstreamSocket, upstreamHead) => {
      stats.upgrades++
      log(`proxy: upgraded ${url.pathname} to a WebSocket`)
      writeHead(reply)
      if (upstreamHead.length) socket.write(upstreamHead)
      if (head.length) upstreamSocket.write(head)
      upstreamSocket.on("error", () => socket.destroy())
      socket.on("close", () => upstreamSocket.destroy())
      upstreamSocket.on("close", () => socket.destroy())
      upstreamSocket.pipe(socket).pipe(upstreamSocket)
    })
    // A refused upgrade: Node has already decoded the body, so send it
    // unframed and end it by closing the connection.
    outbound.on("response", (reply) => {
      log(`proxy: upstream refused the upgrade of ${url.pathname} (HTTP ${reply.statusCode})`)
      writeHead(reply, ["transfer-encoding", "content-length", "connection"], ["Connection: close"])
      reply.pipe(socket)
    })
    outbound.on("error", (error) => {
      log(`proxy: upstream upgrade ${url.pathname} failed: ${error.message}`)
      socket.destroy()
    })
    outbound.end()
  })

  return new Promise((resolve, reject) => {
    server.once("error", reject)
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address()
      const url = `http://127.0.0.1:${port}`
      log(`proxy ${url} -> ${upstream}`)
      resolve({
        url,
        stats,
        heal() {
          failing = false
          log(`proxy: healed after ${stats.failed} failed requests`)
        },
        cutSockets() {
          socketsCut = true
          const open = upgraded.size
          for (const socket of upgraded) socket.destroy()
          log(`proxy: cut ${open} open WebSockets; refusing upgrades`)
        },
        restoreSockets() {
          socketsCut = false
          log(`proxy: restored WebSocket upgrades after refusing ${stats.refusedUpgrades}`)
        },
        close: () => new Promise((done) => {
          for (const socket of upgraded) socket.destroy()
          server.closeAllConnections()
          server.close(() => done())
        }),
      })
    })
  })
}

const JELLYFIN_SEGMENT_TYPES = ["Unknown", "Commercial", "Preview", "Recap", "Outro", "Intro"]
const compactId = (id) => id.replace(/-/g, "").toLowerCase()

// An `answer` hook serving Jellyfin's `GET /MediaSegments/{itemId}` for the
// given items, as Jellyfin 10.10+ does (`MediaSegmentDtoQueryResult`, ids in
// Jellyfin's dashless form, positions in 100 ns ticks), honouring the
// `includeSegmentTypes` filter. `segmentsByItemId` maps an item or media
// source id to `[{ type: "Intro", startS, endS }]`. Other requests pass.
export function mediaSegmentsAnswer(segmentsByItemId) {
  const byId = new Map(Object.entries(segmentsByItemId).map(([id, segments]) => [compactId(id), { id: compactId(id), segments }]))
  return (url, method) => {
    const match = method === "GET" && url.pathname.match(/^\/MediaSegments\/([^/]+)$/i)
    const item = match && byId.get(compactId(decodeURIComponent(match[1])))
    if (!item) return null
    const include = url.searchParams.getAll("includeSegmentTypes").map((type) => type.toLowerCase())
    const items = item.segments
      .filter((segment) => !include.length || include.includes(segment.type.toLowerCase()))
      .map((segment, index) => {
        if (!JELLYFIN_SEGMENT_TYPES.includes(segment.type)) throw new Error(`not a Jellyfin media segment type: ${segment.type}`)
        return {
          Id: `5e6d${item.id.slice(4, 24)}${String(index + 1).padStart(8, "0")}`,
          ItemId: item.id,
          Type: segment.type,
          StartTicks: Math.round(segment.startS * 1e7),
          EndTicks: Math.round(segment.endS * 1e7),
        }
      })
    return { status: 200, body: { Items: items, TotalRecordCount: items.length, StartIndex: 0 } }
  }
}
