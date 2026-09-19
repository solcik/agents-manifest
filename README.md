# agents-manifest

`agent-skills` synchronises pinned skill dependencies into project discovery directories.

Humans can start with the commands and manifest example below.
Contributors must read [CONTRIBUTING.md](CONTRIBUTING.md).
AI agents must read [AGENTS.md](AGENTS.md).
The [documentation index](docs/index.md) links the guides, reference pages, and verification records.

The CLI supports Linux and macOS.
Rust 1.89 is the minimum supported compiler.
Git must be available on PATH.

## Commands

```sh
agent-skills validate
agent-skills plan
agent-skills sync
agent-skills check --offline
agent-skills completions zsh
```

The default manifest is `.agents/skills.yaml`.
The selected root can be a Git worktree or an ordinary directory.
Use `--project "$HOME"` for a HOME manifest.
Use `--project <temporary-directory>` for an ad hoc manifest.
Use `--project` to select another project.
Use `--manifest` to select another manifest within that project.
Use `--worktrees` to publish one manifest into every worktree of a repository.
Validation also accepts a positional manifest path.

## Worktree containers

A container holds a bare Git directory and one worktree per branch.
Every harness stops its skill search at the worktree it starts in.
A container directory therefore cannot hold the projection for its lanes.

```sh
agent-skills --project <container> --worktrees sync
```

The command reads `<container>/.agents/skills.yaml`.
It publishes the same skills into each checked-out worktree.
The bare entry carries no working tree, so the command skips it.
Omit `--worktrees` to publish into one selected lane only.

`validate` checks declarations without Git, network access, or cache creation.
`plan` resolves sources without project writes.
`sync` publishes the validated plan.
`check` returns 5 if generated content or metadata differs.

Use `--json` for versioned reports.
Use `--quiet` to suppress successful output.
Use `--offline` to prohibit source downloads.
Use `--jobs` to limit parallel workers.
Use `--timeout` to limit each Git operation.

## Project manifest

```yaml
version: 1
targets: [codex, claude, pi, opencode]
skills:
  - name: sample
    source: https://github.com/example/skills.git
    revision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    path: skills/sample
bundles: []
```

Replace the example source and revision with an existing repository and commit.
The example demonstrates syntax.
Validation does not check source availability.

Every external source requires a complete immutable commit identifier.
The YAML parser uses YAML 1.1.
Quote revisions and other string values.

Projects track local skills under `.agents/skills/<name>`.
Local skills need no manifest entry.
The CLI never owns or replaces their canonical directories.
If Claude is a project target, synchronisation copies local skills into `.claude/skills`.

Bundle references use `source`, `revision`, and `path`.
Bundle files contain `version: 1` and a `skills` list.
Bundles cannot reference other bundles.
Entry targets can narrow parent targets.
An entry cannot exclude a selected harness that discovers canonical `.agents/skills` files.

## File safety

External skills use regular files under `.agents/skills`.
Claude projections use regular files under `.claude/skills`.
The CLI maintains exact generated-directory patterns in one marked `.gitignore` block.
The CLI preserves text outside that block.

The ignored ownership file records generated paths and content hashes.
It is not a dependency lockfile.
Modified generated output causes a conflict.
Unmanaged directories and tracked output paths cause conflicts.
The CLI never selects a collision winner.

Source trees cannot contain symlinks, submodules, unsafe paths, or filesystem name collisions.
Each skill requires `SKILL.md` with its declared name and a nonempty description.

Synchronisation uses an exclusive project lock.
Read commands use a shared lock if the lock file exists.
Publication journals preserve previous content.
If publication stops, the next `sync` recovers its journal.
If recovery finds user edits, it preserves the output and backup.

The lock coordinates CLI processes.
It does not prevent unrelated editors or harnesses from accessing files.
The CLI rechecks destinations before replacements.
Several directory replacements do not provide one atomic filesystem snapshot.
Git caches are local trusted data.
The CLI verifies cached objects before use.
Static content validation does not perform a prompt injection audit.

## Cache and authentication

The default cache is `$XDG_CACHE_HOME/agents-manifest`.
Without that variable, the CLI uses `$HOME/.cache/agents-manifest`.
Use `--cache-dir` or `AGENTS_MANIFEST_CACHE_DIR` to override the path.

Git reads cached objects without a source checkout.
Each command verifies a source cache once.
Batch object reads avoid one Git process per file.
Independent sources resolve through a bounded worker pool.

Authentication uses the existing Git configuration without interactive prompts.
`--git-mode auto` detects this host's scoped `git agent` wrapper.
The wrapper supports GitHub and the configured VsPoint GitLab host.
Other hosts require a suitable transport environment.
Portable environments use system Git.
The CLI never reads staged credentials or prints raw Git diagnostics.

## Exit status

| Status | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Filesystem or internal failure |
| 2 | Invalid declaration or arguments |
| 3 | Source retrieval or cache failure |
| 4 | Ownership, concurrency, or recovery conflict |
| 5 | Generated content drift |

## Development

```sh
direnv allow .
direnv exec . devenv tasks run quality:lint test:all manifest:validate
direnv exec . devenv test
direnv exec . devenv tasks run bench:core
direnv exec . devenv tasks run bench:cli
```

The devenv environment owns the Rust tools.
Tests use memory adapters and local Git object fixtures.
Tests require no credentials or remote repositories.

Build the binary with `direnv exec . cargo build --release --locked`.
The resulting executable is `target/release/agent-skills`.
The NixOS integration installs this CLI and disables the conflicting legacy host command.
The [HP notebook verification](docs/post-rebuild-summary.md) records the deployed executable and skill discovery tests.

## Release preparation

CI checks Linux with stable Rust and Rust 1.89.
The release workflow builds Linux binaries only for now.
GitHub actions use major-version tags instead of commit hashes.
The Rust toolchain action uses its `stable` branch with an explicit toolchain selection.
Release-plz prepares a release PR from Conventional Commits.
The PR contains the version update and generated changelog.
After approval and merge, automation creates a draft release.
The binary workflow checks, builds, and publishes the release with SHA256SUMS.
Read the [release guide](docs/releases.md) for setup, version rules, and retries.

## Current limits

Hermes targets and `[all]` remain unsupported pending discovery verification.
Version one supports `on-demand` activation only.
Discovery imports and Windows support remain separate work.

Read the [specification](docs/specification.md), [implementation plan](docs/implementation-plan.md), and [design notes](docs/architecture.md).
