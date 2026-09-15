# CLI design

The executable parses arguments and maps errors to exit statuses.
The library contains application behavior.
UV provides an example of this executable boundary. [UV executable](https://github.com/astral-sh/uv/blob/main/crates/uv/src/bin/uv.rs)

## Patterns and responsibilities

`SourceReference` is the shared value object for immutable dependency identity.
Its constructor validates references before it creates an instance.
Private fields prevent callers from invalidating that identity.
Its methods derive cache keys and Git object selectors.
Strict wire structs preserve the manifest format without Serde flatten ambiguity.
`SkillOrigin` distinguishes external references from local source paths without sentinel strings.

`Source` is the gateway boundary for source content.
`GitSource` implements the Git adapter.
Tests replace that adapter with memory sources.
The adapter owns timeout handling, cache verification, and batch object parsing.

`Tree` owns content validation, byte accounting, deterministic hashing, and staged file output.
Tree files share immutable byte buffers through `Arc`.
Canonical output and harness copies share the resolved tree in memory.

`Planner` orchestrates source retrieval and filesystem observations.
`OutputSnapshot::reconcile` is the pure decision boundary.
It maps an observed output and desired content into an optional operation.
The function has no filesystem or network access.
`Operation::change` derives reports from the same operations that publication consumes.

`Transaction` implements the filesystem unit of work.
Its constructor requires an exclusive lock capability.
A shared read lock cannot construct a transaction.
The journal records previous and desired fingerprints before the first replacement.
Recovery infers publication progress from output and backup fingerprints.

## Crates

Clap owns argument validation, help, versions, and completions. [Clap](https://docs.rs/clap/)
Serde owns strict wire decoding and structured output. [Serde](https://serde.rs/)
BLAKE3 owns content fingerprints. [BLAKE3](https://docs.rs/blake3/)
Rayon owns the bounded resolver pool. [Rayon](https://docs.rs/rayon/)
Tempfile owns disposable cache and staging directories. [Tempfile](https://docs.rs/tempfile/)
Thiserror owns typed application errors. [Thiserror](https://docs.rs/thiserror/)

The standard library owns project file locks. [File locks](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
Wait-timeout bounds child lifetimes.
Nix provides safe Unix process-group termination after a timeout.
Unicode-normalization helps detect portable filename collisions.

Proptest tests path invariants.
Jsonschema tests the published editor schema.
Divan measures pure manifest and hashing operations.
These crates are development dependencies.
SHA-1 and zlib create deterministic Git fixture objects without credentialed Git commands.

## Performance choices

The CLI has no async runtime.
Its work consists of bounded blocking Git and filesystem operations.
One resolver pool handles both bundle and skill retrieval.
Per-source locks deduplicate concurrent cache preparation.
Source identity and immutable revision define the cache key.
The selected path does not duplicate the repository cache.

Git lists selected trees once.
One batch process reads unique blobs for a skill tree.
Repeated blob references share byte buffers.
Executable modes survive the projection.
Git never checks out or executes selected source content. [Git batch reads](https://git-scm.com/docs/git-cat-file)

Tree fingerprints include relative paths, file hashes, executable flags, and directory entries.
They ignore timestamps.
BTreeMap gives stable ordering without dependence on filesystem enumeration order.
File hashes stream through a fixed 64 KiB buffer.

The CLI bounds manifest size, file size, tree size, entry count, and worker count.
Resolved project content cannot exceed 512 MiB.
Worker buffers add transient memory above that retained-content limit.
Large sources require suitable memory for the selected worker count.

## Publication limits

The transaction is recoverable across process interruption.
Directory flushes and file flushes support durable journal publication on Unix filesystems.
The CLI cannot guarantee storage behavior outside filesystem durability contracts.
External editors do not participate in CLI locks.
Fingerprint checks detect many concurrent edits.
The CLI preserves conflicting output and backups instead of guessing a recovery winner.

Generated directories form several independent filesystem replacements.
Harnesses can observe intermediate output if they read during publication.
Run synchronisation before a harness session.
