import assert from "node:assert/strict"
import test from "node:test"
import { seerrImageUrl } from "../src/lib/api.ts"

test("Seerr artwork URLs use the requested rendition and repaired cache version", () => {
  assert.equal(
    seerrImageUrl("/matrix poster.jpg", "w154"),
    "/api/seerr/image/w154/matrix%20poster.jpg?v=2",
  )
  assert.equal(seerrImageUrl(null), null)
})
