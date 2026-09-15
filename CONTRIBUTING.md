# Contributing

## Start

Read the [documentation index](docs/index.md).
AI agents must also read [AGENTS.md](AGENTS.md).

Approve the environment once:

```sh
direnv allow .
```

Run the project checks:

```sh
direnv exec . devenv tasks run quality:lint test:all manifest:validate
direnv exec . devenv test
```

Use the declared environment for Rust tools and benchmarks.
Do not introduce a global toolchain requirement.

## Change contract

Add regression tests for changed behavior.
Keep source retrieval separate from reconciliation and publication.
Preserve the ownership, locking, and recovery contracts.
Update the specification when public behavior changes.
Use Conventional Commits for release classification.
The devenv environment installs a `commit-msg` hook through the declared `committed` tool.
Run `direnv exec . devenv tasks run commits:check` to check the current commit.
CI validates new PR commits with the same `committed.toml` policy.
The PR title workflow validates squash commit titles separately.
Use `!` in a PR title to mark a breaking change.
Read the [release guide](docs/releases.md) before release changes.

## Documentation contract

Use ordinary Markdown pages under `docs/`.
Start each page with one descriptive level-one heading.
Use relative links with explicit filenames.
Keep filenames stable after publication.
Avoid generator-specific shortcodes and required plugins.
Keep historical verification records separate from current instructions.
The [index](docs/index.md) defines the initial navigation for a future documentation website.
