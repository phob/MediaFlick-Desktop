import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { createServer } from "node:http"
import { afterEach, describe, it } from "node:test"
import { connect } from "./cdp.mjs"

const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

function encodeFrame(opcode, payload = Buffer.alloc(0)) {
  const header = payload.length < 126
    ? Buffer.from([0x80 | opcode, payload.length])
    : Buffer.from([0x80 | opcode, 126, payload.length >> 8, payload.length & 0xff])
  return Buffer.concat([header, payload])
}

function sendJson(socket, message) {
  socket.write(encodeFrame(0x1, Buffer.from(JSON.stringify(message), "utf8")))
}

function sendClose(socket, code = 1000, reason = "") {
  const reasonBytes = Buffer.from(reason, "utf8")
  const payload = Buffer.alloc(2 + reasonBytes.length)
  payload.writeUInt16BE(code, 0)
  reasonBytes.copy(payload, 2)
  socket.write(encodeFrame(0x8, payload))
}

function parseFrames(buffer, onFrame) {
  let offset = 0
  while (buffer.length - offset >= 2) {
    const opcode = buffer[offset] & 0x0f
    const masked = (buffer[offset + 1] & 0x80) !== 0
    let length = buffer[offset + 1] & 0x7f
    let cursor = offset + 2
    if (length === 126) {
      if (buffer.length - cursor < 2) break
      length = buffer.readUInt16BE(cursor)
      cursor += 2
    }
    const maskLength = masked ? 4 : 0
    if (buffer.length - cursor < maskLength + length) break
    const mask = masked ? buffer.subarray(cursor, cursor + 4) : null
    cursor += maskLength
    const payload = Buffer.from(buffer.subarray(cursor, cursor + length))
    if (mask) {
      for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4]
    }
    onFrame({ opcode, payload })
    offset = cursor + length
  }
  return buffer.subarray(offset)
}

async function startFakeCdp(onMessage) {
  const sockets = new Set()
  const server = createServer((req, res) => {
    if (req.url === "/json/list") {
      const { port } = server.address()
      res.writeHead(200, { "Content-Type": "application/json" })
      res.end(JSON.stringify([{
        type: "page",
        url: "mediaflick-desktop://app/",
        webSocketDebuggerUrl: `ws://127.0.0.1:${port}/devtools/page/1`,
      }]))
      return
    }
    res.writeHead(404).end()
  })
  server.on("upgrade", (req, socket) => {
    sockets.add(socket)
    socket.on("close", () => sockets.delete(socket))
    const accept = createHash("sha1").update(req.headers["sec-websocket-key"] + WS_GUID).digest("base64")
    socket.write(
      "HTTP/1.1 101 Switching Protocols\r\n"
      + "Upgrade: websocket\r\n"
      + "Connection: Upgrade\r\n"
      + `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    )
    let pendingBytes = Buffer.alloc(0)
    socket.on("data", (chunk) => {
      pendingBytes = parseFrames(Buffer.concat([pendingBytes, chunk]), ({ opcode, payload }) => {
        if (opcode === 0x8) {
          if (!socket.writableEnded) {
            sendClose(socket, 1000)
            socket.end()
          }
          return
        }
        if (opcode === 0x1) {
          onMessage(JSON.parse(payload.toString("utf8")), socket)
        }
      })
    })
  })
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve))
  return {
    port: server.address().port,
    sockets,
    close: () => new Promise((resolve) => {
      for (const socket of sockets) socket.destroy()
      server.close(() => resolve())
    }),
  }
}

function rejectAfter(promise, ms) {
  let timer
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`timed out after ${ms} ms`)), ms)
    }),
  ]).finally(() => clearTimeout(timer))
}

describe("cdp.mjs socket close settlement", () => {
  const cleanups = []
  afterEach(async () => {
    while (cleanups.length) await cleanups.pop()()
  })

  it("resolves normal responses and rejects CDP protocol errors", async () => {
    const server = await startFakeCdp((message, socket) => {
      if (message.method === "Test.ok") sendJson(socket, { id: message.id, result: { value: 42 } })
      else sendJson(socket, { id: message.id, error: { message: "boom" } })
    })
    cleanups.push(() => server.close())

    const client = await connect(server.port)
    cleanups.push(() => client.close())

    assert.deepEqual(await client.send("Test.ok"), { value: 42 })
    await assert.rejects(() => client.send("Test.fail"), /Test\.fail: boom/)
  })

  it("rejects all pending requests when the server closes the WebSocket", async () => {
    const received = []
    let activeSocket = null
    const server = await startFakeCdp((message, socket) => {
      activeSocket = socket
      received.push(message)
      if (received.length === 2) {
        sendClose(activeSocket, 1001, "going away")
        activeSocket.end()
      }
    })
    cleanups.push(() => server.close())

    const client = await connect(server.port)
    cleanups.push(() => client.close())

    const first = client.send("Input.dispatchMouseEvent", { type: "mouseReleased" })
    const second = client.send("DOM.getDocument")

    await assert.rejects(() => rejectAfter(first, 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      assert.match(error.message, /Input\.dispatchMouseEvent/)
      return true
    })
    await assert.rejects(() => rejectAfter(second, 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      assert.match(error.message, /DOM\.getDocument/)
      return true
    })
  })

  it("rejects pending requests when the TCP socket drops abruptly", async () => {
    const server = await startFakeCdp((_message, socket) => {
      socket.destroy()
    })
    cleanups.push(() => server.close())

    const client = await connect(server.port)
    cleanups.push(() => client.close())

    const pending = client.send("Runtime.evaluate", { expression: "1" })
    await assert.rejects(() => rejectAfter(pending, 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      assert.match(error.message, /Runtime\.evaluate/)
      return true
    })
  })

  it("rejects send() immediately after the WebSocket closes", async () => {
    const server = await startFakeCdp((_message, socket) => {
      sendClose(socket, 1000)
      socket.end()
    })
    cleanups.push(() => server.close())

    const client = await connect(server.port)
    cleanups.push(() => client.close())

    await assert.rejects(() => rejectAfter(client.send("Page.enable"), 2000), /CdpClosedError|closed/i)
    await assert.rejects(() => rejectAfter(client.send("DOM.enable"), 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      assert.match(error.message, /DOM\.enable/)
      return true
    })
  })

  it("rejects pending and subsequent send() calls when client.close() is called", async () => {
    const server = await startFakeCdp(() => {})
    cleanups.push(() => server.close())

    const client = await connect(server.port)
    const pending = client.send("Page.captureScreenshot")
    client.close()

    await assert.rejects(() => rejectAfter(pending, 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      assert.match(error.message, /Page\.captureScreenshot/)
      return true
    })
    await assert.rejects(() => rejectAfter(client.send("DOM.getDocument"), 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      assert.match(error.message, /DOM\.getDocument/)
      return true
    })
  })

  it("fails waitFor() immediately when the socket closes instead of spinning until timeout", async () => {
    const server = await startFakeCdp((_message, socket) => {
      sendClose(socket, 1001)
      socket.end()
    })
    cleanups.push(() => server.close())

    const client = await connect(server.port)
    cleanups.push(() => client.close())

    await assert.rejects(() => rejectAfter(client.waitFor("false", { timeout: 10000, interval: 50 }), 2000), (error) => {
      assert.equal(error.name, "CdpClosedError")
      return true
    })
  })
})
