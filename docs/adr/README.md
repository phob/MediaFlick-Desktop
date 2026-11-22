# Architecture decision records

Each record states one decision that is hard to reverse, the alternatives it
rejected, and the consequences someone reading the code would not infer. The
code, `BUILDING.md`, `distribution/libmpv/*/README.md` and `AGENTS.md` own the
details and the operative rules; a record links to them instead of repeating
them.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-built-in-libmpv-player.md) | Built-in player is in-process, dynamically loaded libmpv, chosen at startup | Accepted |
| [0002](0002-native-video-composition.md) | mpv presents video natively; CEF is composited over it per platform | Accepted |
| [0003](0003-mpv-json-ipc-and-post-load-resume.md) | Every mpv runtime is driven over one JSON IPC path and resumes after `file-loaded` | Accepted |
| [0004](0004-companion-trust-boundary.md) | Provider credentials and calls live only in the server-side Companion | Accepted |
| [0005](0005-expand-only-persistence.md) | Durable files evolve expand-only, because older releases keep opening them | Accepted |
| [0006](0006-linux-libmpv-runtime-baseline.md) | Linux AppImage libmpv keeps GPU paths and builds on Ubuntu 24.04 | Accepted |

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
