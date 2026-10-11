// A loopback Jellyfin proxy for drives that need a server failing in a
// controlled way. It forwards every request to `upstream` (including its path
// prefix, such as `/stable`) and drops the connection of each request that
// `fail(url, method)` selects, which the app sees as an unreachable server.
// `heal()` stops failing. The app accepts `http://127.0.0.1:<port>` as a server
// address, so a drive signs in to `proxy.url`.

import http from "node:http"
import https from "node:https"

export function startProxy({ upstream, fail, log }) {
  const base = new URL(upstream)
  const prefix = base.pathname.replace(/\/+$/, "")
  const transport = base.protocol === "https:" ? https : http
  const stats = { forwarded: 0, failed: 0 }
  let failing = true

  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://proxy.invalid")
    if (failing && fail(url, request.method ?? "GET")) {
      stats.failed++
      if (stats.failed === 1) log(`proxy: failing ${request.method} ${url.pathname} (first failure)`)
      request.socket.destroy()
      return
    }
    stats.forwarded++
    const headers = { ...request.headers, host: base.host }
    const outbound = transport.request(
      { protocol: base.protocol, hostname: base.hostname, port: base.port || undefined, method: request.method, path: `${prefix}${url.pathname}${url.search}`, headers },
      (answer) => {
        response.writeHead(answer.statusCode ?? 502, answer.headers)
        answer.pipe(response)
      },
    )
    outbound.on("error", (error) => {
      log(`proxy: upstream ${request.method} ${url.pathname} failed: ${error.message}`)
      if (!response.headersSent) response.writeHead(502)
      response.end()
    })
    request.pipe(outbound)
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
        close: () => new Promise((done) => {
          server.closeAllConnections()
          server.close(() => done())
        }),
      })
    })
  })
}
