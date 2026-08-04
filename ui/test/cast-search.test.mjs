import assert from "node:assert/strict"
import test from "node:test"
import {
  castSearchPath,
  readCastSearch,
  writeResolvedCastPerson,
} from "../src/lib/cast-search.ts"
import { libraryItemQuery } from "../src/lib/library-filters.ts"

test("cast mode round-trips stable provider identities and an exact display name", () => {
  const path = castSearchPath({
    jellyfinId: "person/42",
    tmdbId: 6384,
    name: "Keanu Reeves",
  })
  const params = new URL(path, "https://app.test").searchParams

  assert.deepEqual(readCastSearch(params), {
    jellyfinId: "person/42",
    tmdbId: 6384,
    name: "Keanu Reeves",
  })
  assert.equal(params.get("mode"), "person")
})

test("route enrichment preserves navigation state and replaces name-only search identity", () => {
  const previous = new URLSearchParams("mode=person&personName=Alex+Smith&from=detail")
  const next = writeResolvedCastPerson(previous, {
    jellyfinId: "jf-alex-2",
    tmdbId: 123,
    name: "Alex Smith",
    imageTag: null,
  })

  assert.equal(next.get("personId"), "jf-alex-2")
  assert.equal(next.get("tmdbPersonId"), "123")
  assert.equal(next.get("from"), "detail")
  assert.equal(next.has("search"), false)
})

test("ordinary text search remains the existing FTS query and never becomes person mode", () => {
  const params = new URLSearchParams("search=Keanu+Reeves&sort=rating")

  assert.equal(readCastSearch(params), null)
  assert.deepEqual(libraryItemQuery(params), {
    search: "Keanu Reeves",
    kind: "",
    favorite: undefined,
    genre: "",
    decade: undefined,
    sort: "rating",
    watched: "",
  })
})
