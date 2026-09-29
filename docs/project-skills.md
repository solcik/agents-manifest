# Project skill deployment

The repository tracks its dependency declaration in `.agents/skills.yaml`.
The declaration selects `codebase-design` and `writing-guidelines` from an immutable curated repository commit.
The curated repository is private.
Its SSH URL requires repository access through the configured Git transport.
Generated content and ownership metadata remain ignored.

Run `agent-skills updates preview` to inspect upstream skill changes.
The output includes GitHub compare links for GitHub sources.
If a selected change passes review, copy its commits into `agent-skills updates apply --skill NAME --from PIN --to COMMIT`.
Run one apply command for each selected skill.
The apply command keeps comments and unrelated YAML formatting.
Use `branch`, `tag`, or `version` instead of `revision` to follow an upstream selector.
The first apply for a selector omits `--from` and creates `.agents/skills-lock.json`.
Commit that lockfile with the manifest.
Later applies use the old locked commit as `--from`.
The lockfile keeps normal syncs on exact commits and verifies skill tree hashes.
Run the sync task after the pin updates.

Run `direnv exec . devenv tasks run skills:sync` to retrieve and publish the declared skill.
Run `direnv exec . devenv tasks run skills:check` to verify the cached projection without network access.
Start a new agent session after synchronisation.
An existing session can retain an earlier skill catalog.

The verification brief tests startup discovery separately from manual file access.
It requests a design assessment with the installed skill.
The generated result records the agent's evidence.
Global harness skills remain outside the CLI's project policy.
