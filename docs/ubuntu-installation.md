# Install and use agent-skills on Ubuntu

`agent-skills` publishes pinned project skills for Codex, Claude, Pi, and OpenCode.
This guide uses the `v0.4.0` Linux x86-64 release.
The [release page](https://github.com/solcik/agents-manifest/releases/tag/v0.4.0) provides the archive and `SHA256SUMS`.

## Install the published binary

Install the required Ubuntu tools:

```sh
sudo apt-get update
sudo apt-get install -y ca-certificates curl git tar
```

Check the machine architecture before downloading the archive:

```sh
uname -m
```

The following archive requires `x86_64` output.
The release does not provide an ARM binary.
Build from source on another architecture.

```sh
set -e
version=v0.4.0
archive=agent-skills-x86_64-unknown-linux-gnu.tar.gz
mkdir -p "$HOME/Downloads/agent-skills-$version" "$HOME/.local/bin"
cd "$HOME/Downloads/agent-skills-$version"
curl -fL --retry 3 -O "https://github.com/solcik/agents-manifest/releases/download/$version/$archive"
curl -fL --retry 3 -O "https://github.com/solcik/agents-manifest/releases/download/$version/SHA256SUMS"
sha256sum -c SHA256SUMS
tar -xzf "$archive"
install -m 755 agent-skills "$HOME/.local/bin/agent-skills"
export PATH="$HOME/.local/bin:$PATH"
agent-skills --version
```

The checksum command must report `OK` before extraction.
Add the `PATH` export to your shell startup file for later sessions.

The [release workflow](../.github/workflows/release.yaml) builds the archive with `cargo build --locked --release`.
It packages one `agent-skills` executable and publishes `SHA256SUMS`.
The repository does not publish this CLI on crates.io.

## Build from source on another architecture

Use Rust 1.89 or newer and Git.
Check out the release tag, then build its locked dependencies:

```sh
set -e
git clone --branch v0.4.0 --depth 1 https://github.com/solcik/agents-manifest.git
cd agents-manifest
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/agent-skills "$HOME/.local/bin/agent-skills"
export PATH="$HOME/.local/bin:$PATH"
agent-skills --version
```

## Declare project skills

Create `.agents/skills.yaml` at the project root.
This example pins a public skill to a complete Git commit:

```yaml
version: 1
targets: [codex, claude, pi, opencode]
skills:
  - name: grilling
    source: https://github.com/mattpocock/skills.git
    revision: "c55ee46073ed923f86ce59a5eb3b6d895095d1b7"
    path: skills/productivity/grilling
```

Pin each external skill at its source repository.
Quote the revision because the manifest parser uses YAML 1.1.
Store project-owned skills directly under `.agents/skills/<name>/SKILL.md`.
Do not add project-owned skills to the manifest.

Run the following commands from the project root:

```sh
agent-skills validate
agent-skills plan
agent-skills sync
agent-skills check --offline
```

`validate` checks the manifest without network access.
`plan` resolves sources and reports file changes without changing the project.
`sync` publishes external skills under `.agents/skills/`.
It copies skills into `.claude/skills/` when the manifest targets Claude.
The CLI records generated paths in `.agents/skills-state.json` and a marked `.gitignore` block.
Commit the manifest and the marked `.gitignore` block.
Keep generated skill directories and the state file untracked.

The first `plan` or `sync` downloads missing source commits into the local cache.
`check --offline` uses cached sources and makes no network requests.
Exit status 5 means generated content or metadata differs from the manifest.
After a manifest change, run `plan` and `sync` again.

## Publish into every worktree

Run these commands from the worktree that contains the selected manifest:

```sh
agent-skills plan --worktrees --offline
agent-skills sync --worktrees --offline
agent-skills check --worktrees --offline
```

The `--worktrees` flag publishes that one manifest into each checked-out worktree.
The command skips the bare Git directory in a worktree container.
It does not copy the manifest into another branch's Git history.
Run `sync --worktrees` again after adding a worktree.
Omit `--offline` when the source cache lacks a required commit.

## Link a worktree container root

Use this step when agents start in a container root above its worktrees.
First, run `agent-skills sync` in the base worktree.
The base normally holds the remote default branch, such as `main`.
Run these commands from the container root or one of its worktrees:

```sh
agent-skills link
agent-skills link --check
```

`link` points the root's skill directories at the base worktree with relative symlinks.
The root reads future base-worktree syncs through those links.
Pass `--base /path/to/container/other-lane` when another worktree is the intended base.
`link --check` returns status 5 when a link is missing or stale.
`link` returns status 4 if a real directory occupies a link path.
Inspect that directory before removing it.

The [CLI reference](../README.md) describes command options and exit statuses.
