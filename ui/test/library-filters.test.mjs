import assert from "node:assert/strict"
import test from "node:test"
import { api, PAGE_SIZE } from "../src/lib/api.ts"
import {
  activeLibraryFilterCount,
  libraryItemQuery,
  libraryKind,
  libraryKindPath,
  readLibraryFilters,
  releaseDecades,
  writeLibraryFilters,
} from "../src/lib/library-filters.ts"

test("release decades are standard, descending, and span 1900 through the current decade", () => {
  const currentDecade = Math.floor(new Date().getUTCFullYear() / 10) * 10
  const decades = releaseDecades()

  assert.deepEqual(decades[0], { value: currentDecade, label: `${currentDecade}s` })
  assert.deepEqual(decades.at(-1), { value: 1900, label: "1900s" })
  assert.equal(decades.length, (currentDecade - 1900) / 10 + 1)
  assert.ok(decades.every((option, index) => index === 0 || option.value === decades[index - 1].value - 10))
})

test("every library filter round-trips in the URL and paging resets", () => {
  const previous = new URLSearchParams("kind=Movie&search=matrix&offset=60&page=2")
  const written = writeLibraryFilters(previous, {
    sort: "year",
    genre: "Science Fiction",
    decade: "1990",
    watched: "false",
    favorite: true,
  })

  assert.equal(
    written.toString(),
    "kind=Movie&search=matrix&sort=year&genre=Science+Fiction&decade=1990&watched=false&favorite=true",
  )
  assert.deepEqual(readLibraryFilters(written), {
    sort: "year",
    genre: "Science Fiction",
    decade: "1990",
    watched: "false",
    favorite: true,
  })
  assert.equal(activeLibraryFilterCount(readLibraryFilters(written)), 4)

  const query = libraryItemQuery(written)
  assert.deepEqual(query, {
    search: "matrix",
    kind: "Movie",
    favorite: true,
    genre: "Science Fiction",
    decade: 1990,
    sort: "year",
    watched: "false",
  })
})

test("invalid URL filter enums do not become API filters", () => {
  const params = new URLSearchParams("sort=chaos&decade=1995&watched=maybe")
  assert.deepEqual(readLibraryFilters(params), {
    sort: "name",
    genre: "",
    decade: "",
    watched: "",
    favorite: false,
  })
  assert.equal(libraryKind(params), "Movie")
  assert.deepEqual(libraryItemQuery(params), {
    search: "",
    kind: "Movie",
    favorite: undefined,
    genre: "",
    decade: undefined,
    sort: "name",
    watched: "",
  })
})

test("switching Movies and Series uses clean URLs while search and global My List span kinds", () => {
  assert.equal(libraryKindPath("Movie"), "/library?kind=Movie")
  assert.equal(libraryKindPath("Series"), "/library?kind=Series")
  assert.equal(libraryKind(new URLSearchParams("search=matrix")), "")
  assert.equal(libraryKind(new URLSearchParams("favorite=true")), "")
  assert.equal(libraryKind(new URLSearchParams("kind=Series&favorite=true")), "Series")
  assert.equal(libraryKind(new URLSearchParams("kind=Movie%2CSeries&genre=Drama")), "Movie,Series")
  assert.equal(libraryKind(new URLSearchParams("kind=")), "")
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

  assert.equal(requested, "/api/items?kind=Series&decade=2010&limit=60&offset=60")
})
