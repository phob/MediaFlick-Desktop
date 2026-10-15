import assert from "node:assert/strict"
import test from "node:test"
import {
  defaultDetailNavigationState,
  detailNavigationState,
  isSidebarRouteActive,
  librarySearchFromLocation,
  readDetailNavigationState,
} from "../src/lib/navigation.ts"

test("discovery detail routes keep the Discover destination active", () => {
  assert.equal(isSidebarRouteActive("/discover", "/discover"), true)
  assert.equal(isSidebarRouteActive("/discover", "/discover/movie/603"), true)
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

test("a library page is named in the back link, and titles default to their library", () => {
  const libraries = [{ id: "f137a2dd21bbc1b99aa5c0f6bf02a805", name: "Films", kind: "movies", imageTag: null, membershipPending: false, browse: { sorts: ["name"], filters: [], letterIndex: true } }]
  assert.deepEqual(
    detailNavigationState({ pathname: "/library/f137a2dd21bbc1b99aa5c0f6bf02a805", search: "?sort=year" }, "Films"),
    { from: "/library/f137a2dd21bbc1b99aa5c0f6bf02a805?sort=year", label: "Back to Films" },
  )
  assert.deepEqual(
    defaultDetailNavigationState({ layout: "title", libraryId: "f137a2dd21bbc1b99aa5c0f6bf02a805" }, libraries),
    { from: "/library/f137a2dd21bbc1b99aa5c0f6bf02a805", label: "Back to Films" },
  )
  assert.deepEqual(
    defaultDetailNavigationState({ layout: "title", libraryId: null }, libraries),
    { from: "/", label: "Back to home" },
  )
})

test("detail return state rejects external and malformed targets", () => {
  assert.equal(readDetailNavigationState({ from: "https://example.com", label: "Back" }), null)
  assert.equal(readDetailNavigationState({ from: "//example.com", label: "Back" }), null)
  assert.equal(readDetailNavigationState({ from: "/library", label: "" }), null)
})
