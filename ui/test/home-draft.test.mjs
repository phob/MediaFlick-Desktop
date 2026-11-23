import assert from "node:assert/strict"
import test from "node:test"
import { homeElementKey, reconcileHomeDraft } from "../src/routes/settings/home-draft.ts"

function makeElement(kind, id, enabled, overrides = {}) {
  return {
    kind,
    id,
    enabled,
    label: id,
    available: true,
    category: kind === "builtIn" ? "Built-in" : kind === "library" ? "Library" : "Genre",
    ...overrides,
  }
}

function makeConfig(elements, overrides = {}) {
  return {
    billboard: true,
    watching: { continueWatching: true, nextUp: true, combine: true },
    elements,
    ...overrides,
  }
}

test("reconcileHomeDraft preserves reordered elements and toggles across background adds and removes", () => {
  const previous = makeConfig([
    makeElement("builtIn", "watching", true),
    makeElement("genre", "Action", true),
    makeElement("genre", "Drama", false),
  ])
  const draft = makeConfig([
    makeElement("genre", "Action", false),
    makeElement("builtIn", "watching", true),
    makeElement("genre", "Drama", false),
  ], { billboard: false })
  const source = makeConfig([
    makeElement("builtIn", "watching", true),
    makeElement("genre", "Action", true, { label: "Action Movies" }),
    makeElement("genre", "Comedy", false),
  ], { watching: { continueWatching: true, nextUp: false, combine: true } })

  const merged = reconcileHomeDraft(previous, draft, source)
  assert.deepEqual(merged, {
    billboard: false,
    watching: { continueWatching: true, nextUp: false, combine: true },
    elements: [
      makeElement("genre", "Action", false, { label: "Action Movies" }),
      makeElement("builtIn", "watching", true),
      makeElement("genre", "Comedy", false),
    ],
  })
})

test("reconcileHomeDraft adopts server order when only enabled state changed", () => {
  const previous = makeConfig([
    makeElement("builtIn", "watching", true),
    makeElement("genre", "Action", true),
  ])
  const draft = makeConfig([
    makeElement("builtIn", "watching", true),
    makeElement("genre", "Action", false),
  ])
  const source = makeConfig([
    makeElement("builtIn", "watching", true),
    makeElement("library", "lib1", true),
    makeElement("genre", "Action", true),
  ])

  const merged = reconcileHomeDraft(previous, draft, source)
  assert.deepEqual(
    merged.elements.map((element) => [homeElementKey(element), element.enabled]),
    [
      ["builtIn:watching", true],
      ["library:lib1", true],
      ["genre:Action", false],
    ],
  )
})

test("invariant: reconcileHomeDraft always matches the exact server key set and is idempotent", () => {
  let seed = 0x51a7_29c3
  const nextInt = (max) => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
    return seed % max
  }
  const shuffle = (items) => {
    const copy = [...items]
    for (let i = copy.length - 1; i > 0; i--) {
      const j = nextInt(i + 1)
      ;[copy[i], copy[j]] = [copy[j], copy[i]]
    }
    return copy
  }

  const pool = [
    ["builtIn", "watching"],
    ["builtIn", "myList"],
    ["library", "lib1"],
    ["library", "lib2"],
    ["genre", "Action"],
    ["genre", "Comedy"],
    ["genre", "Drama"],
    ["genre", "Horror"],
  ]

  for (let trial = 0; trial < 200; trial++) {
    const prevPool = shuffle(pool).slice(0, 2 + nextInt(pool.length - 1))
    const previous = makeConfig(prevPool.map(([kind, id]) => makeElement(kind, id, nextInt(2) === 1)))
    const editedOrder = nextInt(2) === 1
    const draftElements = (editedOrder ? shuffle(previous.elements) : previous.elements).map((element) => (
      nextInt(3) === 0 ? { ...element, enabled: !element.enabled } : { ...element }
    ))
    const draft = makeConfig(draftElements, { billboard: nextInt(2) === 1 })
    const sourcePool = shuffle(pool).slice(0, 2 + nextInt(pool.length - 1))
    const source = makeConfig(sourcePool.map(([kind, id]) => makeElement(kind, id, nextInt(2) === 1)))

    const result = reconcileHomeDraft(previous, draft, source)
    const resultKeys = result.elements.map(homeElementKey)
    const sourceKeys = source.elements.map(homeElementKey)

    assert.equal(resultKeys.length, sourceKeys.length, `trial=${trial}`)
    assert.deepEqual([...resultKeys].sort(), [...sourceKeys].sort(), `trial=${trial}`)
    assert.deepEqual(reconcileHomeDraft(source, result, source), result, `trial=${trial} idempotent`)
  }
})
