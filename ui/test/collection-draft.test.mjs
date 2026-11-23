import assert from "node:assert/strict"
import test from "node:test"
import {
  buildCollectionSettingsPatch,
  executeCollectionSave,
  planCollectionSave,
  reconcileCollectionDraft,
  reconcileDeletedProfileIds,
  reconcileProfileIds,
  removeProfilesFromIndex,
  savedCollectionDraft,
  upsertProfileInIndex,
} from "../src/routes/settings/collection-draft.ts"

function draft(overrides = {}) {
  return {
    modeSelection: "mediaFlick",
    includeUnreleased: false,
    profileIds: ["a", "b"],
    deletedProfileIds: [],
    ...overrides,
  }
}

function makeProfile(id, title = `Collection ${id}`) {
  return {
    id,
    revision: "r".repeat(16),
    template: { id: "tmdb.discover.movie.popular" },
    title,
    description: "",
    customPosterId: null,
    source: { kind: "tmdbDiscover", parameters: {} },
    mediaType: "movie",
    limit: { kind: "all" },
    cadence: "daily",
    availableOnHome: false,
  }
}

function makeSettings(overrides = {}) {
  return {
    effectiveMode: "mediaFlick",
    mediaFlickAvailable: true,
    modeSelection: null,
    franchises: { includeUnreleased: false },
    readiness: { tmdb: true, mdblist: true },
    recovery: null,
    ...overrides,
  }
}

test("reconcileProfileIds preserves user relative order and appends newly added server ids", () => {
  assert.deepEqual(
    reconcileProfileIds(["a", "b"], ["b", "a"], ["a", "b", "c"]),
    ["b", "a", "c"],
  )
  assert.deepEqual(
    reconcileProfileIds(["a", "b"], ["b", "a"], ["a", "b", "c", "d"]),
    ["b", "a", "c", "d"],
  )
})

test("reconcileProfileIds drops server-removed ids and handles simultaneous adds and removes", () => {
  assert.deepEqual(
    reconcileProfileIds(["a", "b", "c"], ["c", "b", "a"], ["a", "c"]),
    ["c", "a"],
  )
  assert.deepEqual(
    reconcileProfileIds(["a", "b"], ["b", "a"], ["a"]),
    ["a"],
  )
  assert.deepEqual(
    reconcileProfileIds(["a", "b", "c"], ["c", "a", "b"], ["a", "c", "d"]),
    ["c", "a", "d"],
  )
})

test("reconcileProfileIds adopts server order when draft order was untouched", () => {
  assert.deepEqual(
    reconcileProfileIds(["a", "b"], ["a", "b"], ["b", "a"]),
    ["b", "a"],
  )
  assert.deepEqual(
    reconcileProfileIds(["a", "b"], ["a", "b"], ["a", "b", "c"]),
    ["a", "b", "c"],
  )
})

test("reconcileDeletedProfileIds drops server-removed ids and deduplicates", () => {
  assert.deepEqual(reconcileDeletedProfileIds(["a", "b"], ["b", "c"]), ["b"])
  assert.deepEqual(reconcileDeletedProfileIds(["a"], ["b"]), [])
  assert.deepEqual(reconcileDeletedProfileIds(["a", "a", "b"], ["a", "b"]), ["a", "b"])
})

test("reconcileCollectionDraft rebases profileIds, prunes deletedProfileIds, and merges scalar edits", () => {
  const previous = draft({ profileIds: ["a", "b"], deletedProfileIds: [] })
  const current = draft({
    includeUnreleased: true,
    profileIds: ["b", "a"],
    deletedProfileIds: ["a", "b"],
  })
  const source = draft({
    modeSelection: "jellyfin",
    includeUnreleased: false,
    profileIds: ["b", "c"],
    deletedProfileIds: [],
  })

  assert.deepEqual(reconcileCollectionDraft(previous, current, source), {
    modeSelection: "jellyfin",
    includeUnreleased: true,
    profileIds: ["b", "c"],
    deletedProfileIds: ["b"],
  })
})

test("planCollectionSave plans full-list reorder only when surviving order changes and never pins unchanged mode", () => {
  const baseline = draft({ profileIds: ["a", "b", "c"] })

  const reorderedWithNew = planCollectionSave(
    draft({ profileIds: ["b", "a", "c"] }),
    baseline,
  )
  assert.deepEqual(reorderedWithNew, {
    reorderIds: ["b", "a", "c"],
    deletions: [],
    settingsPatch: null,
  })

  const deleteOnlyPreservesSurvivingOrder = planCollectionSave(
    draft({ profileIds: ["b", "a", "c"], deletedProfileIds: ["a"] }),
    baseline,
  )
  assert.deepEqual(deleteOnlyPreservesSurvivingOrder, {
    reorderIds: null,
    deletions: ["a"],
    settingsPatch: null,
  })

  const deleteAndReorderSurviving = planCollectionSave(
    draft({ profileIds: ["a", "c", "b"], deletedProfileIds: ["a"] }),
    baseline,
  )
  assert.deepEqual(deleteAndReorderSurviving, {
    reorderIds: ["a", "c", "b"],
    deletions: ["a"],
    settingsPatch: null,
  })

  assert.equal(
    buildCollectionSettingsPatch(draft(), draft()),
    null,
  )
})

test("removeProfilesFromIndex and upsertProfileInIndex keep profiles and errors consistent", () => {
  const a = makeProfile("a", "Alpha")
  const b = makeProfile("b", "Beta")
  const c = makeProfile("c", "Gamma")
  const index = { profiles: [a, b], errors: { a: "broken", b: "warn" } }

  assert.deepEqual(removeProfilesFromIndex(index, ["a"]), {
    profiles: [b],
    errors: { b: "warn" },
  })
  assert.deepEqual(upsertProfileInIndex(index, c), {
    profiles: [a, b, c],
    errors: { a: "broken", b: "warn" },
  })
  const renamedB = { ...b, title: "Beta Edited" }
  assert.deepEqual(upsertProfileInIndex(index, renamedB), {
    profiles: [a, renamedB],
    errors: { a: "broken", b: "warn" },
  })
})

test("executeCollectionSave runs reorder before deletes and settings PATCH so reorder failure prevents destructive deletes", async () => {
  const calls = []
  const a = makeProfile("a")
  const b = makeProfile("b")
  const c = makeProfile("c")
  const settings = makeSettings()
  const profiles = { profiles: [a, b, c], errors: {} }
  const baseline = savedCollectionDraft(settings, profiles)
  const value = {
    ...baseline,
    includeUnreleased: true,
    profileIds: ["a", "c", "b"],
    deletedProfileIds: ["a"],
  }

  const outcome = await executeCollectionSave(value, baseline, settings, profiles, {
    reorderProfiles: async (ids) => {
      calls.push(["reorder", ids])
      throw new Error("the collection order is incomplete")
    },
    deleteProfile: async (id) => {
      calls.push(["delete", id])
      return { deleted: true }
    },
    patchSettings: async (patch) => {
      calls.push(["patch", patch])
      return settings
    },
    fetchSettings: async () => settings,
    fetchProfiles: async () => profiles,
  })

  assert.equal(outcome.ok, false)
  assert.deepEqual(calls, [["reorder", ["a", "c", "b"]]])
  assert.deepEqual(outcome.committedDeletions, [])
  assert.equal(outcome.committedReorder, false)
  assert.equal(outcome.committedSettings, false)
})

test("executeCollectionSave records committed reorder and committed deletions before a later failure", async () => {
  const calls = []
  const a = makeProfile("a")
  const b = makeProfile("b")
  const c = makeProfile("c")
  const d = makeProfile("d")
  const settings = makeSettings()
  const profiles = { profiles: [a, b, c, d], errors: { a: "err-a", d: "err-d" } }
  const baseline = savedCollectionDraft(settings, profiles)
  const value = {
    ...baseline,
    includeUnreleased: true,
    profileIds: ["a", "b", "d", "c"],
    deletedProfileIds: ["a", "b"],
  }

  const outcome = await executeCollectionSave(value, baseline, settings, profiles, {
    reorderProfiles: async (ids) => {
      calls.push(["reorder", ids])
      return { profiles: [a, b, d, c], errors: profiles.errors }
    },
    deleteProfile: async (id) => {
      calls.push(["delete", id])
      if (id === "b") throw new Error("delete b failed")
      return { deleted: true }
    },
    patchSettings: async (patch) => {
      calls.push(["patch", patch])
      return settings
    },
    fetchSettings: async () => settings,
    fetchProfiles: async () => ({ profiles: [b, d, c], errors: { d: "err-d" } }),
  })

  assert.equal(outcome.ok, false)
  assert.deepEqual(calls, [
    ["reorder", ["a", "b", "d", "c"]],
    ["delete", "a"],
    ["delete", "b"],
  ])
  assert.equal(outcome.committedReorder, true)
  assert.deepEqual(outcome.committedDeletions, ["a"])
  assert.equal(outcome.committedSettings, false)
  assert.deepEqual(outcome.profiles, {
    profiles: [b, d, c],
    errors: { d: "err-d" },
  })
})

test("invariant: reconcileCollectionDraft always yields a permutation of server ids and subset for deletedProfileIds", () => {
  let seed = 0x1a2b_3c4d
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

  const universe = ["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"]

  for (let trial = 0; trial < 250; trial++) {
    const previousIds = shuffle(universe).slice(0, nextInt(universe.length + 1))
    const editedOrder = nextInt(4) !== 0
    const draftIds = editedOrder ? shuffle(previousIds) : [...previousIds]
    const serverIds = shuffle(universe).slice(0, nextInt(universe.length + 1))
    const draftDeleted = shuffle(universe).slice(0, nextInt(5))
    if (draftDeleted.length > 0 && nextInt(2) === 0) {
      draftDeleted.push(draftDeleted[0])
    }

    const previous = draft({ profileIds: previousIds, deletedProfileIds: [] })
    const current = draft({ profileIds: draftIds, deletedProfileIds: draftDeleted })
    const source = draft({ profileIds: serverIds, deletedProfileIds: [] })

    const result = reconcileCollectionDraft(previous, current, source)
    assert.ok(result !== null)

    const serverSet = new Set(serverIds)
    assert.equal(result.profileIds.length, serverIds.length, `trial=${trial} length`)
    assert.equal(new Set(result.profileIds).size, serverIds.length, `trial=${trial} unique`)
    for (const id of result.profileIds) {
      assert.ok(serverSet.has(id), `trial=${trial} unknown id ${id}`)
    }

    if (editedOrder && JSON.stringify(draftIds) !== JSON.stringify(previousIds)) {
      const survivingDraft = draftIds.filter((id) => serverSet.has(id))
      const addedServer = serverIds.filter((id) => !new Set(draftIds).has(id))
      assert.deepEqual(
        result.profileIds,
        [...survivingDraft, ...addedServer],
        `trial=${trial} relative order and appended additions`,
      )
    } else if (JSON.stringify(draftIds) === JSON.stringify(previousIds)) {
      assert.deepEqual(result.profileIds, serverIds, `trial=${trial} untouched adopts server order`)
    }

    assert.equal(
      new Set(result.deletedProfileIds).size,
      result.deletedProfileIds.length,
      `trial=${trial} deleted unique`,
    )
    for (const id of result.deletedProfileIds) {
      assert.ok(serverSet.has(id), `trial=${trial} deleted id ${id} exists on server`)
    }

    const idempotent = reconcileCollectionDraft(source, result, source)
    assert.deepEqual(idempotent, result, `trial=${trial} idempotent`)

    const plan = planCollectionSave(result, source)
    if (plan.reorderIds !== null) {
      assert.equal(plan.reorderIds.length, serverIds.length, `trial=${trial} reorder complete`)
      assert.equal(new Set(plan.reorderIds).size, serverIds.length, `trial=${trial} reorder unique`)
      for (const id of plan.reorderIds) {
        assert.ok(serverSet.has(id), `trial=${trial} reorder valid id ${id}`)
      }
    }
  }
})
