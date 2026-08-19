---
name: rust-unslop
description: Enforce readable, idiomatic, ownership-aware Rust in MediaFlick Desktop. Use whenever Codex creates, edits, refactors, reviews, or debugs Rust source, Rust tests, Cargo configuration, Clippy policy, build.rs, or Rust toolchain files in this repository.
---

# Rust Unslop

Keep Rust changes direct, domain-shaped, and easy to verify. Follow the repository's `AGENTS.md` first; use this skill for the additional Rust-specific workflow below.

## Before editing

- Trace the data flow and identify the narrowest owning module.
- State the invariant or behavior the change must preserve.
- Prefer an existing concrete type or boundary over a new abstraction.
- Check both supported player backends when playback behavior is involved.

## While editing

- Model states and transitions with named types and enums. Do not encode domain state in strings, boolean argument lists, or nested anonymous containers.
- Keep ownership visible. Do not add `clone`, `Arc`, `Mutex`, allocation, or collection merely to satisfy the borrow checker; first shorten borrows or move work to the owner.
- Drop lock guards and other significant temporaries before networking, callbacks, logging, sleeps, or unrelated work.
- Return recoverable failures through the existing error boundary. Do not add `unwrap`, `expect`, `todo!`, `unimplemented!`, or debugging macros to production code.
- Split long or cognitively dense functions by cohesive responsibility. Give helpers domain names; do not create numbered phases or one-use abstraction layers.
- Keep FFI and protocol mechanics in their adapters. Keep shared policy in the existing playback, library, Jellyfin, or Companion owner.
- Write comments for invariants, safety, protocol constraints, and surprising tradeoffs. Do not narrate ordinary syntax.
- Avoid speculative traits, builders, configuration, compatibility shims, and dependencies.
- Keep every enabled lint at error severity. Fix owned diagnostics; do not add lint suppressions or lower a rule to land a change.

## Verify

1. Run `cargo fmt --all` after Rust edits.
2. Run the narrowest relevant tests while iterating.
3. Run `just rust-quality` before handoff.
4. Run `just test` for substantive behavior, ownership, concurrency, persistence, or protocol changes.
5. Run `git diff --check` and review the final diff for needless clones, widened ownership, boolean state, hidden lock lifetimes, and comments that repeat the code.

If a strict lint exposes a legitimate external contract, preserve the wire shape while introducing a clearer internal type or ownership boundary. Do not silence the lint at the DTO, FFI, or test boundary.
