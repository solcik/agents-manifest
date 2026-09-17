# Project skill synchronisation specification

Status: implemented core version one.
Hermes discovery and host installation remain deferred.

Date: 2026-09-15.

## Ownership

The portable Rust CLI owns project policy and file generation.
The NixOS repository owns host installation and global skill integration.
The existing `solcik/agent-skills` repository remains the curated content source.
The public CLI repository is `solcik/agents-manifest`.

The project tracks `.agents/skills.yaml` and local `.agents/skills/<name>` directories.
The manifest contains every external dependency and its immutable Git commit.
The project does not track downloaded content or a separate dependency lockfile.
The selected root can be a Git worktree, HOME, or an ordinary temporary directory.
Git roots retain tracking checks for generated paths.
Ordinary directories retain ownership, locking, and recovery checks without Git tracking.
Broken Git metadata fails before publication.

## Manifest

The schema lives in `schemas/agent-skills-manifest.schema.json`.
Version one requires `version`, `targets`, and `skills`.
The optional `bundles` field contains pinned bundle references.
Unknown fields fail validation.

Each skill declares `name`, `source`, `revision`, and `path`.
Each bundle declares `source`, `revision`, and `path`.
Sources use complete HTTPS or SSH Git URLs.
Revisions use complete 40-character or 64-character hexadecimal commit identifiers.
Paths remain relative to the source repository.
Absolute paths, parent components, empty components, and backslashes fail validation.

Targets are `codex`, `claude`, `pi`, `opencode`, and `hermes`.
The explicit singleton target list `[all]` selects every supported target.
An entry's optional targets must form a subset of the project targets.
The default activation is `on-demand`.
Version one rejects `always` before downloads or file changes.

Bundle files use `version: 1` and a `skills` list with the same skill entry schema.
Bundles cannot reference other bundles.
Bundle entries cannot widen their reference's effective targets.

## Planning and adapters

The planner expands bundles and validates the complete desired projection.
Pure reconciliation functions derive operations from observed outputs and desired content.
Git adapters retrieve immutable content.
Filesystem adapters inspect ownership and apply the validated plan.
Harness adapters define paths and supported activation modes.
The planner does not execute source scripts or source hooks.

Codex, Pi, and OpenCode discover canonical `.agents/skills` files.
Claude receives copied files under `.claude/skills`.
Hermes support requires verification against the pinned project discovery implementation.
Until verification passes, a Hermes target fails with an unsupported-target error.
The `all` expansion also fails if any adapter remains unsupported.

Canonical discovery creates a harness limitation.
Codex, Pi, and OpenCode can discover canonical skills despite narrower entry targets.
Version one rejects target combinations that require hiding canonical content from these harnesses.
The CLI describes targets as projections, not an isolation or context whitelist guarantee.
Global harness skills remain outside project policy.

## Ownership and file safety

The ignored `.agents/skills-state.json` records generated paths, source identities, and content hashes.
The state file is ownership metadata, not a dependency lockfile.
The CLI never adopts an existing directory without recorded ownership.
Local skills and external skills share one name namespace.
Duplicate names fail before publication, even when their targets differ.

Generated ignore patterns name exact managed directories.
The CLI edits only its marked block in each relevant `.gitignore`.
The CLI refuses tracked external output paths.
The CLI refuses modified generated content unless the desired content already matches it.
The CLI never removes local directories or unmanaged files.

Source trees cannot contain symlinks, submodules, device files, or paths outside the selected skill root.
Every selected root must contain a regular `SKILL.md` file.
Its frontmatter name must equal the declared name.
The CLI treats skill text as untrusted content.
Static validation does not constitute a prompt injection audit.
Manifest review authorises the pinned content; synchronisation does not execute it.

The CLI validates every dependency before changing project outputs.
It stages files under a temporary directory on the same filesystem.
It acquires an exclusive project lock before publication.
A journal records each replacement and preserves recoverable previous content.
Interrupted runs recover the journal before another mutation.
Ownership metadata updates only after all projections succeed.

## Commands

`agent-skills validate` checks manifest structure and project policy without network access.
`agent-skills plan` resolves pinned sources and prints proposed changes without project writes.
`agent-skills sync` applies a complete validated plan.
`agent-skills check` compares expected files with recorded ownership and current content.
`agent-skills check --offline` requires every pinned source in the local cache.

Success returns exit status 0.
Invalid declarations return 2.
Source retrieval failures return 3.
Ownership conflicts return 4.
Detected drift returns 5.
Unexpected internal failures return 1.
Errors identify the manifest field or relative project path.
Errors never print credentials or credential-bearing URLs.

Private sources use the configured Git transport without interactive prompts.
On this host, the transport invokes the scoped `git agent` wrapper.
The CLI never reads staged secret files.
Unavailable sources fail the entire operation before publication.

## Distribution and integration

The first release provides Linux binaries.
macOS builds remain deferred for now.
CI builds binaries and publishes checksums after a release request.
Windows support follows filesystem and transport tests.
Discovery imports and skills.sh integration follow the core synchronisation release.
Shell entry never downloads skills.
An opt-in devenv task invokes `agent-skills sync`.

The existing Home Manager shell command retains its behavior until migration tests pass.
Integration must give host mute commands a distinct name before installing the portable CLI as `agent-skills`.
This specification does not change the running host.

## Acceptance criteria

1. Invalid manifests fail before network access.
2. Unpinned sources and nested bundles fail validation.
3. Local and external name collisions preserve every existing project file.
4. Retrieval failures preserve the previous complete projection.
5. Repeated synchronisation produces identical files and ownership hashes.
6. Removing a declaration removes only unmodified owned output.
7. A modified generated file produces a conflict without data loss.
8. A symlink escape fails before publication.
9. An interrupted publication restores or completes the recorded transaction.
10. Offline checks detect content drift without downloading sources.
11. Unsupported activation and target isolation fail with explicit diagnostics.
12. Source tests use local Git repositories and require no live credentials.
