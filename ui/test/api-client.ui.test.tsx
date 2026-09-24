import { afterEach, expect, test, vi } from "vitest"
import { api, ApiError } from "@/lib/api"

afterEach(() => vi.unstubAllGlobals())

function respond(body: string, status: number) {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(body, { status })))
}

test.each([
  ["JSON requests", () => api.status()],
  ["uploads", () => api.collections.uploadArtwork(new ArrayBuffer(1))],
])("%s surface the error envelope and the session-expiry flag", async (_, call) => {
  respond(JSON.stringify({ error: "the Jellyfin session expired", expired: true }), 401)
  const error = await call().catch((failure: unknown) => failure)
  expect(error).toBeInstanceOf(ApiError)
  expect(error).toMatchObject({ message: "the Jellyfin session expired", status: 401, expired: true })
})

test.each([
  ["JSON requests", () => api.status()],
  ["uploads", () => api.collections.uploadArtwork(new ArrayBuffer(1))],
])("%s keep the status when a failure has no JSON body", async (_, call) => {
  respond("<html>bad gateway</html>", 502)
  const error = await call().catch((failure: unknown) => failure)
  expect(error).toBeInstanceOf(ApiError)
  expect(error).toMatchObject({ status: 502, expired: false })
})
