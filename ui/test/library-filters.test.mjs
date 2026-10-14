import assert from "node:assert/strict"
import test from "node:test"
import { api, PAGE_SIZE } from "../src/lib/api.ts"
import {
  libraryItemQuery,
  libraryKind,
  readLibraryFilters,
  writeLibraryFilters,
} from "../src/lib/library-filters.ts"

test("every library filter round-trips in the URL", () => {
  const previous = new URLSearchParams("kind=Movie&search=matrix")
  const written = writeLibraryFilters(previous, {
    sort: "year",
    genre: "Science Fiction",
    decade: "1990",
    watched: "unwatched",
    favorite: true,
  })

  assert.deepEqual(readLibraryFilters(written), {
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

  const query = libraryItemQuery(written)
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
  assert.deepEqual(readLibraryFilters(params), {
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
  assert.deepEqual(libraryItemQuery(params), {
    search: "",
    kind: "Movie",
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

test("search, global Favorites, and mixed Home genre links span kinds", () => {
  assert.equal(libraryKind(new URLSearchParams("search=matrix")), "")
  assert.equal(libraryKind(new URLSearchParams("favorite=true")), "")
  assert.equal(libraryKind(new URLSearchParams("kind=Series&favorite=true")), "Series")
  assert.equal(libraryKind(new URLSearchParams("kind=Movie%2CSeries&genre=Drama")), "Movie,Series")
  assert.equal(
    libraryItemQuery(new URLSearchParams("kind=Movie%2CSeries&genre=Drama")).kind,
    "Movie,Series",
  )
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
