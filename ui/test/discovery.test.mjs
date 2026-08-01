import assert from "node:assert/strict"
import test from "node:test"
import {
  RELEASE_CENTURIES,
  readDiscoveryFilters,
  writeDiscoveryFilters,
} from "../src/lib/discovery.ts"

test("century and genre survive URL serialization together", () => {
  const original = new URLSearchParams("row=movies&library=outside")
  const written = writeDiscoveryFilters(original, "movies", {
    genre: 18,
    century: 20,
    sort: "rating",
    minRating: 7,
  })

  assert.equal(written.get("genre"), "18")
  assert.equal(written.get("century"), "20")
  assert.equal(written.get("library"), "outside")
  assert.deepEqual(readDiscoveryFilters(written, "movies", true, true), {
    genre: 18,
    century: 20,
    sort: "rating",
    minRating: 7,
  })
})

test("unsupported centuries do not become discovery API filters", () => {
  assert.deepEqual(
    readDiscoveryFilters(new URLSearchParams("genre=18&century=18"), "movies", true, true),
    { genre: 18, sort: "popular" },
  )
  assert.deepEqual(
    readDiscoveryFilters(new URLSearchParams("century=19"), "tv", true, true),
    { sort: "popular" },
  )
  assert.deepEqual(
    readDiscoveryFilters(new URLSearchParams("century=20"), "movies", true, false),
    { sort: "popular" },
  )
})

test("switching discovery rows resets catalogue filters but keeps local state", () => {
  const original = new URLSearchParams(
    "row=movies&genre=18&century=20&sort=rating&library=outside",
  )
  original.set("row", "tv")
  const written = writeDiscoveryFilters(original, "tv", { sort: "popular" })

  assert.equal(written.toString(), "row=tv&library=outside")
})

test("available century labels reflect film and television release history", () => {
  assert.deepEqual(RELEASE_CENTURIES.movie.map(({ value }) => value), [21, 20, 19])
  assert.deepEqual(RELEASE_CENTURIES.tv.map(({ value }) => value), [21, 20])
  assert.match(RELEASE_CENTURIES.movie[1].label, /1901–2000/)
})
