import assert from "node:assert/strict"
import test from "node:test"
import {
  isSidebarRouteActive,
  librarySearchFromLocation,
} from "../src/lib/navigation.ts"

test("discovery detail routes keep the Discover destination active", () => {
  assert.equal(isSidebarRouteActive("/discover", "/discover"), true)
  assert.equal(isSidebarRouteActive("/discover", "/discover/movie/603"), true)
  assert.equal(isSidebarRouteActive("/requests", "/requests/12"), false)
})

test("sidebar search follows the library URL and clears outside it", () => {
  assert.equal(
    librarySearchFromLocation("/library", "?search=The%20Matrix&favorite=true"),
    "The Matrix",
  )
  assert.equal(librarySearchFromLocation("/discover", "?search=The%20Matrix"), "")
  assert.equal(librarySearchFromLocation("/library", "?kind=Movie"), "")
})
