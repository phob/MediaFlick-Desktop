import assert from "node:assert/strict"
import test from "node:test"
import {
  RELEASE_DECADES,
  readDiscoveryFilters,
  writeDiscoveryFilters,
} from "../src/lib/discovery.ts"

test("decade and genre survive URL serialization together", () => {
  const original = new URLSearchParams("row=movies&library=outside")
  const written = writeDiscoveryFilters(original, "movies", {
    genre: 18,
    decade: 1990,
    sort: "rating",
    minRating: 7,
  })

  assert.equal(written.get("genre"), "18")
  assert.equal(written.get("decade"), "1990")
  assert.equal(written.get("library"), "outside")
  assert.deepEqual(readDiscoveryFilters(written, "movies", true, true), {
    genre: 18,
    decade: 1990,
    sort: "rating",
    minRating: 7,
  })
})

test("unsupported decades do not become discovery API filters", () => {
  assert.deepEqual(
    readDiscoveryFilters(new URLSearchParams("genre=18&decade=1995"), "movies", true, true),
    { genre: 18, sort: "popular" },
  )
  assert.deepEqual(
    readDiscoveryFilters(new URLSearchParams("decade=1890"), "tv", true, true),
    { sort: "popular" },
  )
  assert.deepEqual(
    readDiscoveryFilters(new URLSearchParams("decade=1990"), "movies", true, false),
    { sort: "popular" },
  )
})

test("switching discovery rows resets catalogue filters but keeps local state", () => {
  const original = new URLSearchParams(
    "row=movies&genre=18&decade=1990&sort=rating&library=outside",
  )
  original.set("row", "tv")
  const written = writeDiscoveryFilters(original, "tv", { sort: "popular" })

  assert.equal(written.toString(), "row=tv&library=outside")
})

test("available decade labels reflect release history and stop at the current decade", () => {
  const currentDecade = Math.floor(new Date().getUTCFullYear() / 10) * 10
  assert.equal(RELEASE_DECADES.movie[0].value, currentDecade)
  assert.equal(RELEASE_DECADES.movie.at(-1).value, 1800)
  assert.equal(RELEASE_DECADES.tv.at(-1).value, 1900)
  assert.equal(
    RELEASE_DECADES.movie.find(({ value }) => value === 1990)?.label,
    "1990s (1990–1999)",
  )
  assert.match(RELEASE_DECADES.movie[0].label, /present/)
})

test("a previously serialized century is removed rather than reinterpreted", () => {
  const legacy = new URLSearchParams("row=movies&century=20&genre=18&library=outside")
  assert.deepEqual(readDiscoveryFilters(legacy, "movies", true, true), {
    genre: 18,
    sort: "popular",
  })

  const written = writeDiscoveryFilters(legacy, "movies", {
    genre: 18,
    sort: "popular",
  })
  assert.equal(written.has("century"), false)
  assert.equal(written.toString(), "row=movies&library=outside&genre=18")
})
