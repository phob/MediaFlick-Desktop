import assert from "node:assert/strict"
import test from "node:test"
import {
  RELEASE_DECADES,
  discoveryCardKey,
  discoveryResultSetKey,
  discoveryResultsForSet,
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
  assert.equal(RELEASE_DECADES.movie.at(-1).value, 1900)
  assert.equal(RELEASE_DECADES.tv.at(-1).value, 1900)
  assert.equal(RELEASE_DECADES.movie.find(({ value }) => value === 1980)?.label, "80s")
  assert.equal(RELEASE_DECADES.movie.find(({ value }) => value === 1990)?.label, "90s")
  assert.equal(RELEASE_DECADES.movie.find(({ value }) => value === 1970)?.label, "1970s")
  assert.equal(RELEASE_DECADES.movie.find(({ value }) => value === 2000)?.label, "2000s")
  assert.equal(RELEASE_DECADES.movie[0].label, `${currentDecade}s`)
})

test("every discovery criterion replaces the complete infinite result-set identity", () => {
  const original = discoveryResultSetKey("movies", { sort: "popular" }, "all")
  const alternatives = [
    discoveryResultSetKey("movies", { genre: 18, sort: "popular" }, "all"),
    discoveryResultSetKey("movies", { decade: 1980, sort: "popular" }, "all"),
    discoveryResultSetKey("movies", { sort: "rating" }, "all"),
    discoveryResultSetKey("movies", { minRating: 7, sort: "popular" }, "all"),
    discoveryResultSetKey("movies", { sort: "popular" }, "outside"),
    discoveryResultSetKey("movies", { sort: "popular" }, "library"),
  ]

  assert.equal(new Set([original, ...alternatives]).size, alternatives.length + 1)

  // Even a title shared by the old unfiltered page and the new 1980s page is
  // remounted. Four leading cards can therefore never keep old card/poster
  // state while later cards reconcile to the replacement page.
  const result = { mediaType: "movie", tmdbId: 1 }
  const eighties = alternatives[1]
  assert.notEqual(discoveryCardKey(original, result), discoveryCardKey(eighties, result))
})

test("obsolete leading pages cannot mix into a replacement decade", () => {
  const unfiltered = discoveryResultSetKey("movies", { sort: "popular" })
  const eighties = discoveryResultSetKey("movies", { sort: "popular", decade: 1980 })
  const staleLeadingCards = Array.from({ length: 4 }, (_, index) => ({
    mediaType: "movie",
    tmdbId: index + 1,
    title: `Future title ${index + 1}`,
    year: 2026,
  }))
  const currentCards = [
    { mediaType: "movie", tmdbId: 80, title: "Eighties one", year: 1984 },
    { mediaType: "movie", tmdbId: 81, title: "Eighties two", year: 1989 },
  ]

  const visible = discoveryResultsForSet(eighties, [
    { resultSetKey: unfiltered, results: staleLeadingCards },
    { resultSetKey: eighties, results: currentCards },
  ])

  assert.deepEqual(visible, currentCards)
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
