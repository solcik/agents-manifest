# Project skill deployment

The repository tracks its dependency declaration in `.agents/skills.yaml`.
The declaration selects `codebase-design` and `writing-guidelines` from an immutable curated repository commit.
The curated repository is private.
Its SSH URL requires repository access through the configured Git transport.
Generated content and ownership metadata remain ignored.

Run `direnv exec . devenv tasks run skills:sync` to retrieve and publish the declared skill.
Run `direnv exec . devenv tasks run skills:check` to verify the cached projection without network access.
Start a new agent session after synchronisation.
An existing session can retain an earlier skill catalog.

The verification brief tests startup discovery separately from manual file access.
It requests a design assessment with the installed skill.
The generated result records the agent's evidence.
Global harness skills remain outside the CLI's project policy.
