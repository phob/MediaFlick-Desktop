import assert from "node:assert/strict"
import test from "node:test"
import { api, PAGE_SIZE } from "../src/lib/api.ts"
import {
  libraryItemQuery,
  libraryQuery,
  readLibraryFilters,
  sortsFor,
  writeLibraryFilters,
} from "../src/lib/library-filters.ts"

const TITLES = {
  sorts: ["name", "year", "added", "rating", "played", "random"],
  filters: ["genre", "decade", "watched", "parental", "favorite"],
  letterIndex: true,
}
const MUSIC = { sorts: ["name", "added", "year"], filters: ["favorite"], letterIndex: false }

test("every library filter round-trips in the URL", () => {
  const previous = new URLSearchParams("kind=Movie&search=matrix")
  const written = writeLibraryFilters(previous, {
    sort: "year",
    genre: "Science Fiction",
    decade: "1990",
    watched: "unwatched",
    favorite: true,
  }, TITLES)

  assert.deepEqual(readLibraryFilters(written, TITLES), {
    sort: "year",
    order: "desc",
    seed: "",
    genre: "Science Fiction",
    decade: "1990",
    watched: "unwatched",
    parental: "",
    seriesStatus: "",
    favorite: true,
  })

  const query = libraryItemQuery(written, TITLES)
  assert.deepEqual(query, {
    search: "matrix",
    kind: "Movie",
    favorite: true,
    genre: "Science Fiction",
    decade: 1990,
    sort: "year",
    order: undefined,
    seed: undefined,
    watched: "unwatched",
    parental: "",
    seriesStatus: "",
  })
})

test("invalid URL filter enums do not become API filters", () => {
  const params = new URLSearchParams("sort=chaos&decade=1995&watched=maybe")
  assert.deepEqual(readLibraryFilters(params, TITLES), {
    sort: "name",
    order: "asc",
    seed: "",
    genre: "",
    decade: "",
    watched: "",
    parental: "",
    seriesStatus: "",
    favorite: false,
  })
  assert.deepEqual(libraryItemQuery(params, TITLES), {
    search: "",
    kind: "",
    favorite: undefined,
    genre: "",
    decade: undefined,
    sort: "name",
    order: undefined,
    seed: undefined,
    watched: "",
    parental: "",
    seriesStatus: "",
  })
})

test("the title view keeps catalog kinds from old links and lists both without one", () => {
  assert.equal(libraryItemQuery(new URLSearchParams("search=matrix"), TITLES).kind, "")
  assert.equal(libraryItemQuery(new URLSearchParams("kind=Series&favorite=true"), TITLES).kind, "Series")
  assert.equal(libraryItemQuery(new URLSearchParams("kind=Movie%2CSeries&genre=Drama"), TITLES).kind, "Movie,Series")
  assert.equal(libraryItemQuery(new URLSearchParams("kind=MusicAlbum"), TITLES).kind, "")
})

test("a library offers and sends only what its capabilities list", () => {
  assert.deepEqual(sortsFor(MUSIC).map((sort) => sort.id), ["name", "added", "year"])
  const params = new URLSearchParams("sort=random&genre=Rock&watched=unwatched&favorite=true")
  assert.deepEqual(libraryQuery(params, MUSIC), {
    favorite: true,
    genre: "",
    decade: undefined,
    sort: "name",
    order: undefined,
    seed: undefined,
    watched: "",
    parental: "",
    seriesStatus: "",
  })
  const written = writeLibraryFilters(params, { sort: "added" }, MUSIC)
  assert.equal(written.toString(), "sort=added&favorite=true")
})

test("the items API sends decade and page bounds to the server", async () => {
  const originalFetch = globalThis.fetch
  let requested = ""
  globalThis.fetch = async (url) => {
    requested = String(url)
    return new Response(JSON.stringify({ items: [], total: 0 }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    })
  }

  try {
    await api.items({ kind: "Series", decade: 2010, limit: PAGE_SIZE, offset: PAGE_SIZE })
  } finally {
    globalThis.fetch = originalFetch
  }

  const url = new URL(requested, "https://app.test")
  assert.equal(url.pathname, "/api/items")
  assert.deepEqual(Object.fromEntries(url.searchParams), {
    kind: "Series",
    decade: "2010",
    limit: String(PAGE_SIZE),
    offset: String(PAGE_SIZE),
  })
})
