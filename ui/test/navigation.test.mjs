import assert from "node:assert/strict"
import test from "node:test"
import {
  defaultDetailNavigationState,
  detailNavigationState,
  isSidebarRouteActive,
  librarySearchFromLocation,
  readDetailNavigationState,
} from "../src/lib/navigation.ts"

test("direct library details always return to the matching library", () => {
  assert.deepEqual(defaultDetailNavigationState("Movie"), {
    from: "/library?kind=Movie",
    label: "Back to library",
  })
  assert.deepEqual(defaultDetailNavigationState("Episode"), {
    from: "/library?kind=Series",
    label: "Back to library",
  })
})

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

test("detail links carry a safe, labelled return destination", () => {
  assert.deepEqual(
    detailNavigationState({ pathname: "/library", search: "?kind=Series&sort=year" }),
    { from: "/library?kind=Series&sort=year", label: "Back to library" },
  )
  assert.deepEqual(
    detailNavigationState({
      pathname: "/item/season-1",
      state: { from: "/calendar", label: "Back to releases" },
    }),
    { from: "/calendar", label: "Back to releases" },
  )
})

test("detail return state rejects external and malformed targets", () => {
  assert.equal(readDetailNavigationState({ from: "https://example.com", label: "Back" }), null)
  assert.equal(readDetailNavigationState({ from: "//example.com", label: "Back" }), null)
  assert.equal(readDetailNavigationState({ from: "/library", label: "" }), null)
})
