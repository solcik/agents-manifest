# HP notebook deployment verification

Date: 2026-09-16.
Execution host: `solcik-notebook-hp`.
The NixOS main checkout contains merge commit `cd315ff5b47879a36368ee35fa36d26cdf35ee2b`.
The active system identifies the HP notebook.

## Installed CLI

The host profile provides `agent-skills 0.1.0`.
Its executable resolves to `/nix/store/ilrkwbvm3jbzairhrrjx55h6fn4l4h70-agent-skills-0.1.0-unstable-2026-09-15/bin/agent-skills`.
The installed CLI passed manifest validation, synchronisation, and an offline check.
The synchronisation and check reports contained no changes.

The coordinator created an empty source cache under the ignored project `.devenv` directory.
The installed CLI retrieved the immutable private source through scoped SSH transport.
The plan reported no projection changes.
The subsequent offline check passed with no changes.
This test verifies fresh retrieval separately from existing cache verification.
The verification cache remains under `.devenv/verification-cache.VGFetY`.

## Global controls

The three managed global discovery roots are absent.
The roots are `~/.agents/skills`, `~/.claude/skills`, and `~/.config/opencode/skills`.
The curated repository remains intact.
The host profile exposes no legacy reconciliation, mute-list, or audit executable.
The persistent Codex configuration disables bundled skills and installed plugin skill paths.
The test uses fresh sessions after the configuration update. [Official Codex skill guidance](https://learn.chatgpt.com/docs/build-skills)

Hermes has its native bundled opt-out marker.
Its configuration disables 72 nonessential skill names.
Its external global skill directory list is empty.
Its essential `hermes-agent` manual remains present and enabled.
The user approved that exception.
No harness source patch forms part of this deployment.
This verification does not assert an empty Claude plugin catalog or a live Hermes session catalog.

## Fresh agent evidence

Both agents used Codex, `gpt-5.6-sol`, and low reasoning.
Neither session used temporary skill configuration overrides.
The project agent discovered `codebase-design` and `writing-guidelines` at project paths.
It read the complete design skill and its required references.
It used the skill to assess a validated source identity invariant.
It verified the installed CLI and passed an offline check.
Its [result](post-rebuild-verification-result.md) records the command output and source evidence.

The second agent started in the NixOS repository.
Its startup catalog contained no skills.
It independently confirmed the three global roots are absent.
Its [result](post-rebuild-global-catalog-result.md) separates catalog absence from filesystem evidence.
