# Architecture decision records

Each record states one decision that is hard to reverse, the alternatives it
rejected, and the consequences someone reading the code would not infer. The
code, `BUILDING.md`, `distribution/libmpv/*/README.md` and `AGENTS.md` own the
details and the operative rules; a record links to them instead of repeating
them.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-built-in-libmpv-player.md) | Built-in player is in-process, dynamically loaded libmpv, chosen at startup | Superseded by [0017](0017-out-of-process-linux-libmpv-helper.md) |
| [0002](0002-native-video-composition.md) | mpv presents video natively; CEF is composited over it per platform | Accepted (Windows); Linux superseded by [0018](0018-native-wayland-zero-copy-composition.md) |
| [0003](0003-mpv-json-ipc-and-post-load-resume.md) | Every mpv runtime is driven over one JSON IPC path and resumes after `file-loaded` | Accepted |
| [0004](0004-companion-trust-boundary.md) | Provider credentials and calls live only in the server-side Companion | Accepted |
| [0005](0005-expand-only-persistence.md) | `library.db` evolves expand-only because older releases keep opening it; JSON formats only add defaulted fields | Accepted |
| [0006](0006-linux-libmpv-runtime-baseline.md) | Linux AppImage libmpv keeps GPU paths and builds on Ubuntu 24.04 | Accepted |
| [0007](0007-cef-custom-scheme-and-bridge-boundary.md) | Embedded UI runs on a custom CEF scheme with strict origin, CSP, and thread boundaries | Accepted |
| [0008](0008-thin-sqlite-catalog-and-three-pass-sync.md) | Local SQLite is a thin catalog index refreshed by three-pass sync while rich metadata stays live | Accepted |
| [0009](0009-type-enforced-account-scoping-and-session-lifecycle.md) | Account scoping, startup lifecycle, and single-writer persistence are enforced structurally | Accepted |
| [0010](0010-generated-wire-contracts-and-embedded-ui-build.md) | Wire contracts are generated from Rust and the UI bundle is owned by Cargo builds | Accepted |
| [0011](0011-jellyfin-protocol-compatibility-and-quirks.md) | Jellyfin client accommodates server version floors, header-only auth, and API quirks | Accepted |
| [0012](0012-companion-providers-seerr-and-letterboxd.md) | Companion provider integrations, Seerr joins, and direct public Letterboxd RSS | Accepted |
| [0013](0013-playback-coordinator-and-mpv-runtime-constraints.md) | Playback coordinator actor model and mpv runtime constraints | Accepted |
| [0014](0014-embedded-book-reader-and-archive-wasm.md) | Embedded book reader uses unbundled foliate-js, no-crypto libarchive.wasm, and query-param ranges | Accepted |
| [0015](0015-react-ui-rendering-caching-and-interaction.md) | React UI caching, virtualization, portal isolation, and interaction patterns | Accepted |
| [0016](0016-release-packaging-and-verification-harness.md) | Release state machine, v0.1.6 migration, packaging, and isolated E2E verification | Accepted |
| [0017](0017-out-of-process-linux-libmpv-helper.md) | Out-of-process Linux built-in libmpv helper and bounded native player shutdown | Accepted |
| [0018](0018-native-wayland-zero-copy-composition.md) | Native Wayland subsurface video composition and DMA-BUF CEF UI on Linux | Accepted |

## When to add a record

Add one when a change makes or reverses a decision that is expensive to undo
and whose reason the code cannot show: a rejected alternative someone is likely
to propose again, a constraint imposed by released versions or upstream
software, or a consequence that only appears in production. Do not add one for
a choice the code makes obvious, a build step, or a list of options.

Number records in sequence and never renumber. To change a decision, add a new
record that supersedes the old one, and set the old one's status to
`Superseded by NNNN`; only fix factual errors in place.

## Template

```markdown
# NNNN. Decision as a short sentence

Status: Accepted (YYYY-MM-DD). Decided in <commit or PR>.

## Context

The forces and constraints, including any that come from released versions.

## Decision

What was chosen, in a few sentences.

## Rejected alternatives

- Alternative: why it was rejected.

## Consequences

What follows that the code does not show, including risks and the planned
response if one materializes.
```
