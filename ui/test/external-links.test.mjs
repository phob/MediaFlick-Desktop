import assert from "node:assert/strict"
import test from "node:test"
import { discoveryExternalLinksFor, externalLinksFor } from "../src/lib/api.ts"

test("library movies expose exact information pages from their provider ids", () => {
  const links = externalLinksFor({
    kind: "Movie",
    name: "The Matrix",
    year: 1999,
    providerIds: { tmdb: "603", imdb: "tt0133093", tvdb: "169" },
  })

  assert.deepEqual(links.map((link) => link.id), [
    "imdb",
    "tmdb",
    "tvdb",
    "letterboxd",
    "trakt",
    "rotten-tomatoes-search",
  ])
  assert.equal(links.find((link) => link.id === "letterboxd")?.source, "tmdb")
  assert.equal(links.find((link) => link.id === "trakt")?.source, "imdb")
  assert.equal(
    links.find((link) => link.id === "rotten-tomatoes-search")?.href,
    "https://www.rottentomatoes.com/search?search=The%20Matrix%201999",
  )
})

test("discovered movies link to exact databases and Rotten Tomatoes search", () => {
  const links = discoveryExternalLinksFor({
    mediaType: "movie",
    tmdbId: 603,
    title: "The Matrix",
    year: 1999,
    externalIds: { imdb: "tt0133093", tvdb: null },
  })

  assert.deepEqual(links, [
    { id: "imdb", label: "IMDb", href: "https://www.imdb.com/title/tt0133093/" },
    { id: "tmdb", label: "TMDB", href: "https://www.themoviedb.org/movie/603" },
    { id: "letterboxd", label: "Letterboxd", href: "https://letterboxd.com/tmdb/603" },
    { id: "trakt", label: "Trakt", href: "https://trakt.tv/movies/tt0133093" },
    {
      id: "rotten-tomatoes-search",
      label: "Rotten Tomatoes",
      actionLabel: "Search Rotten Tomatoes",
      href: "https://www.rottentomatoes.com/search?search=The%20Matrix%201999",
    },
  ])
})

test("discovered series link to TV databases and Rotten Tomatoes search without Letterboxd", () => {
  const links = discoveryExternalLinksFor({
    mediaType: "tv",
    tmdbId: 95396,
    title: "Severance",
    year: 2022,
    externalIds: { imdb: "tt11280740", tvdb: 371980 },
  })

  assert.deepEqual(links, [
    { id: "imdb", label: "IMDb", href: "https://www.imdb.com/title/tt11280740/" },
    { id: "tmdb", label: "TMDB", href: "https://www.themoviedb.org/tv/95396" },
    { id: "tvdb", label: "TVDB", href: "https://thetvdb.com/dereferrer/series/371980" },
    { id: "trakt", label: "Trakt", href: "https://trakt.tv/shows/tt11280740" },
    {
      id: "rotten-tomatoes-search",
      label: "Rotten Tomatoes",
      actionLabel: "Search Rotten Tomatoes",
      href: "https://www.rottentomatoes.com/search?search=Severance%202022",
    },
  ])
})

test("invalid external ids never become launched URLs", () => {
  const libraryLinks = externalLinksFor({
    kind: "Series",
    name: "Severance",
    year: 2022,
    providerIds: { tmdb: "0", imdb: "603?title=wrong", tvdb: "not-a-number" },
  })
  assert.deepEqual(libraryLinks, [{
    id: "rotten-tomatoes-search",
    label: "Rotten Tomatoes",
    actionLabel: "Search Rotten Tomatoes",
    href: "https://www.rottentomatoes.com/search?search=Severance%202022",
  }])

  const discoveryLinks = discoveryExternalLinksFor({
    mediaType: "tv",
    tmdbId: 95396,
    title: "Severance",
    year: 2022,
    externalIds: { imdb: "https://example.test", tvdb: -1 },
  })
  assert.deepEqual(discoveryLinks, [
    { id: "tmdb", label: "TMDB", href: "https://www.themoviedb.org/tv/95396" },
    {
      id: "rotten-tomatoes-search",
      label: "Rotten Tomatoes",
      actionLabel: "Search Rotten Tomatoes",
      href: "https://www.rottentomatoes.com/search?search=Severance%202022",
    },
  ])
})

test("older Companion details without external ids retain TMDB and Rotten Tomatoes links", () => {
  const links = discoveryExternalLinksFor({
    mediaType: "tv",
    tmdbId: 95396,
    title: "Severance",
    year: null,
  })
  assert.deepEqual(links, [
    { id: "tmdb", label: "TMDB", href: "https://www.themoviedb.org/tv/95396" },
    {
      id: "rotten-tomatoes-search",
      label: "Rotten Tomatoes",
      actionLabel: "Search Rotten Tomatoes",
      href: "https://www.rottentomatoes.com/search?search=Severance",
    },
  ])
})
