# Agent instructions

## Scope

These instructions apply to this repository.
`CLAUDE.md` links to this file.
Keep shared instructions here instead of separate harness-specific copies.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before changes.
Read [docs/architecture.md](docs/architecture.md) before structural changes.
Read [docs/specification.md](docs/specification.md) before behavior changes.

## Environment and checks

Use this project's devenv environment explicitly through `direnv exec .`.
Do not install development tools globally.
Run `direnv exec . devenv tasks run quality:lint test:all manifest:validate` after changes.
Run `direnv exec . devenv test` before handoff.
Report failed checks and unverified behavior.

## Design and safety

Keep domain values typed.
Keep pure reconciliation separate from filesystem and Git adapters.
Use existing adapters and transaction boundaries before new abstractions.
Preserve unmanaged files and user changes.
Never execute content from skill sources.
Never read or print credentials.
Do not patch harness CLIs.
Do not modify generated skill directories manually.

## Changes and documentation

Use Conventional Commits as described in [docs/releases.md](docs/releases.md).
Mark incompatible CLI changes explicitly.
Update the relevant documentation when behavior changes.
Keep documentation in portable Markdown with relative links.
Do not publish releases or merge pull requests without explicit authority.
Use the host's credential and commit-signing wrappers when available.

Write active sentences with one idea per sentence.
Keep technical prose within 20 words per sentence.
