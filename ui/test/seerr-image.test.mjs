import assert from "node:assert/strict"
import test from "node:test"
import { seerrImageUrl } from "../src/lib/api.ts"

test("Seerr artwork URLs use the native image route with the requested rendition", () => {
  assert.ok(
    seerrImageUrl("/matrix poster.jpg", "w154").startsWith("/api/seerr/image/w154/matrix%20poster.jpg"),
  )
  assert.equal(seerrImageUrl(null), null)
})
