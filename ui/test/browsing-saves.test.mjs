import assert from "node:assert/strict"
import test from "node:test"
import { createLatestSaveQueue } from "../src/lib/browsing-saves.ts"

test("createLatestSaveQueue serializes overlapping saves and coalesces intermediate writes", async () => {
  const started = []
  const resolvers = []
  const settled = []
  const enqueue = createLatestSaveQueue((value) => new Promise((resolve) => {
    started.push(value)
    resolvers.push(resolve)
  }))

  enqueue({ value: "first", onSettled: () => settled.push("first") })
  enqueue({ value: "second", onSettled: () => settled.push("second") })
  enqueue({ value: "third", onSettled: () => settled.push("third") })

  assert.deepEqual(started, ["first"])
  resolvers.shift()()
  await Promise.resolve()
  await Promise.resolve()

  assert.deepEqual(started, ["first", "third"])
  assert.deepEqual(settled, [])

  resolvers.shift()()
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(settled, ["third"])
})

test("createLatestSaveQueue reports error only when no newer save supersedes it", async () => {
  const errors = []
  const settled = []
  let rejectFirst = null
  let resolveSecond = null
  const enqueue = createLatestSaveQueue((value) => new Promise((resolve, reject) => {
    if (value === "first") rejectFirst = reject
    else resolveSecond = resolve
  }))

  enqueue({ value: "first", onError: (error) => errors.push(error.message) })
  enqueue({ value: "second", onSettled: () => settled.push("second") })

  rejectFirst(new Error("transient"))
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(errors, [])

  resolveSecond()
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(settled, ["second"])
})
