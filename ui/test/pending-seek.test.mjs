import assert from "node:assert/strict"
import test from "node:test"
import {
  PENDING_SEEK_TTL_MS,
  clampSeekPosition,
  reconcilePendingSeek,
  setPendingSeekTarget,
  stepPendingSeek,
} from "../src/lib/pending-seek.ts"

function player(overrides = {}) {
  return {
    active: true,
    playbackId: 1,
    itemId: "movie-1",
    nowPlaying: { kind: "video" },
    positionMs: 10_000,
    durationMs: 7_200_000,
    skipSegments: [],
    ...overrides,
  }
}

test("rapid consecutive seeks accumulate over lagged backend position", () => {
  const lagged = player({ positionMs: 10_000 })
  let pending = null
  const targets = []
  for (let i = 0; i < 6; i++) {
    const next = stepPendingSeek(pending, lagged, 30_000, 1_000 + i * 20)
    pending = next.pending
    targets.push(next.targetMs)
  }
  assert.deepEqual(targets, [40_000, 70_000, 100_000, 130_000, 160_000, 190_000])
  assert.equal(pending?.targetMs, 190_000)
})

test("stale ticks and intermediate seek arrivals do not drop later presses", () => {
  let state = stepPendingSeek(null, player({ positionMs: 10_000 }), 30_000, 1_000)
  state = stepPendingSeek(state.pending, player({ positionMs: 10_080 }), 30_000, 1_050)
  state = stepPendingSeek(state.pending, player({ positionMs: 10_150 }), 30_000, 1_100)
  assert.equal(state.targetMs, 100_000)

  const afterIntermediate = reconcilePendingSeek(state.pending, player({ positionMs: 40_050 }), 1_250)
  assert.equal(afterIntermediate?.targetMs, 100_000)

  const fourth = stepPendingSeek(afterIntermediate, player({ positionMs: 40_050 }), 30_000, 1_260)
  assert.equal(fourth.targetMs, 130_000)
})

test("small 1-second seek intervals do not falsely catch up on stale origin or intermediate ticks", () => {
  const first = stepPendingSeek(null, player({ positionMs: 10_000 }), 1_000, 1_000)
  assert.equal(first.targetMs, 11_000)
  assert.notEqual(first.pending, null)

  const afterStale = reconcilePendingSeek(first.pending, player({ positionMs: 10_150 }), 1_050)
  assert.equal(afterStale?.targetMs, 11_000)

  const second = stepPendingSeek(afterStale, player({ positionMs: 10_150 }), 1_000, 1_060)
  assert.equal(second.targetMs, 12_000)

  const third = stepPendingSeek(second.pending, player({ positionMs: 10_150 }), 1_000, 1_070)
  assert.equal(third.targetMs, 13_000)

  const afterIntermediate = reconcilePendingSeek(third.pending, player({ positionMs: 12_000 }), 1_150)
  assert.equal(afterIntermediate?.targetMs, 13_000)

  const whileUndispatched = reconcilePendingSeek(third.pending, player({ positionMs: 13_050 }), 1_180, false)
  assert.equal(whileUndispatched?.targetMs, 13_000)

  const caughtUp = reconcilePendingSeek(third.pending, player({ positionMs: 13_050 }), 1_200, true)
  assert.equal(caughtUp, null)
})

test("net-zero burst stays pending until dispatched and catches up immediately once settled", () => {
  const start = player({ positionMs: 10_000 })
  const forward = stepPendingSeek(null, start, 30_000, 1_000)
  assert.equal(forward.targetMs, 40_000)
  const back = stepPendingSeek(forward.pending, start, -30_000, 1_020)
  assert.equal(back.targetMs, 10_000)
  assert.notEqual(back.pending, null)

  assert.equal(
    reconcilePendingSeek(back.pending, player({ positionMs: 10_050 }), 1_040, false)?.targetMs,
    10_000,
  )
  assert.equal(
    reconcilePendingSeek(back.pending, player({ positionMs: 40_050 }), 1_060, true)?.targetMs,
    10_000,
  )
  assert.equal(
    reconcilePendingSeek(back.pending, player({ positionMs: 10_100 }), 1_080, true),
    null,
  )
})

test("mixed forward and backward seeks clamp at boundaries and reverse immediately", () => {
  const nearStart = player({ positionMs: 5_000, durationMs: 120_000 })
  const back1 = stepPendingSeek(null, nearStart, -10_000, 1_000)
  assert.equal(back1.targetMs, 0)
  const back2 = stepPendingSeek(back1.pending, nearStart, -10_000, 1_020)
  assert.equal(back2.targetMs, 0)
  const forward = stepPendingSeek(back2.pending, nearStart, 30_000, 1_040)
  assert.equal(forward.targetMs, 30_000)

  const nearEnd = player({ positionMs: 110_000, durationMs: 120_000 })
  const fwd1 = stepPendingSeek(null, nearEnd, 30_000, 2_000)
  assert.equal(fwd1.targetMs, 120_000)
  const fwd2 = stepPendingSeek(fwd1.pending, nearEnd, 30_000, 2_020)
  assert.equal(fwd2.targetMs, 120_000)
  const backAfterEnd = stepPendingSeek(fwd2.pending, nearEnd, -10_000, 2_040)
  assert.equal(backAfterEnd.targetMs, 110_000)
})

test("pending seek clears on catch-up, session change, stop, segment skip, out-of-band jump, and expiry", () => {
  const base = player({
    positionMs: 10_000,
    skipSegments: [{ triggered: false }],
  })
  const first = stepPendingSeek(null, base, 30_000, 1_000)
  assert.equal(first.targetMs, 40_000)
  assert.equal(reconcilePendingSeek(first.pending, base, 1_100)?.targetMs, 40_000)

  assert.equal(reconcilePendingSeek(first.pending, player({ positionMs: 40_200 }), 1_300), null)
  assert.equal(reconcilePendingSeek(first.pending, player({ active: false, positionMs: 10_000 }), 1_100), null)
  assert.equal(reconcilePendingSeek(first.pending, player({ playbackId: 2, positionMs: 10_000 }), 1_100), null)
  assert.equal(reconcilePendingSeek(first.pending, player({ itemId: "movie-2", positionMs: 10_000 }), 1_100), null)
  assert.equal(
    reconcilePendingSeek(
      stepPendingSeek(null, player({ nowPlaying: { kind: "audio", playlistItemId: "a" } }), 30_000, 1_000).pending,
      player({ nowPlaying: { kind: "audio", playlistItemId: "b" } }),
      1_100,
    ),
    null,
  )
  assert.equal(
    reconcilePendingSeek(first.pending, player({ positionMs: 25_000, skipSegments: [{ triggered: true }] }), 1_100),
    null,
  )
  assert.equal(reconcilePendingSeek(first.pending, player({ positionMs: 97_500 }), 1_100), null)
  assert.equal(reconcilePendingSeek(first.pending, base, 1_000 + PENDING_SEEK_TTL_MS), null)
})

test("manual absolute seek replaces any pending burst and becomes the base for subsequent relative seeks", () => {
  const lagged = player({ positionMs: 10_000, durationMs: 600_000 })
  const burst = stepPendingSeek(null, lagged, 30_000, 1_000)
  assert.equal(burst.targetMs, 40_000)

  const manual = setPendingSeekTarget(lagged, 300_000, 1_050)
  assert.equal(manual.targetMs, 300_000)
  assert.equal(manual.pending?.targetMs, 300_000)

  const afterManual = stepPendingSeek(manual.pending, lagged, 30_000, 1_100)
  assert.equal(afterManual.targetMs, 330_000)
})

test("clamping handles unknown duration and non-finite inputs", () => {
  assert.equal(clampSeekPosition(-5_000, null), 0)
  assert.equal(clampSeekPosition(45_000, null), 45_000)
  assert.equal(clampSeekPosition(45_000, 0), 45_000)
  assert.equal(clampSeekPosition(Number.NaN, 120_000), 0)
  assert.equal(clampSeekPosition(Number.POSITIVE_INFINITY, 120_000), 0)
})

test("invariant: any generated burst matches sequential clamped arithmetic under lagged ticks", () => {
  let seed = 0x51a7_29c3
  const nextInt = (max) => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
    return seed % max
  }

  for (let trial = 0; trial < 200; trial++) {
    const durationMs = 15_000 + nextInt(3_600_000)
    const startMs = nextInt(durationMs + 1)
    let expectedMs = startMs
    let pending = null
    let now = 10_000
    const steps = 1 + nextInt(12)

    for (let step = 0; step < steps; step++) {
      const magnitude = (1 + nextInt(60)) * 1_000
      const deltaMs = nextInt(2) === 0 ? magnitude : -magnitude
      expectedMs = Math.max(0, Math.min(durationMs, expectedMs + deltaMs))
      now += 15 + nextInt(120)
      const laggedPositionMs = Math.min(durationMs, startMs + step * 40)
      const result = stepPendingSeek(
        pending,
        player({ positionMs: laggedPositionMs, durationMs }),
        deltaMs,
        now,
        false,
      )
      pending = result.pending
      assert.equal(result.targetMs, expectedMs, `trial=${trial} step=${step} delta=${deltaMs}`)
      assert.ok(result.targetMs >= 0 && result.targetMs <= durationMs)
    }
    if (pending) {
      assert.equal(
        reconcilePendingSeek(pending, player({ positionMs: expectedMs, durationMs }), now + 50, true),
        null,
        `trial=${trial} should clear once settled at target`,
      )
    }
  }
})
