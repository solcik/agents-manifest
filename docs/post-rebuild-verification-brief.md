# Post-rebuild verification

Route: Codex, openai-codex, gpt-5.6-sol, low reasoning.
The user requested the verification phase after rebuilding the HP notebook.
The session has no temporary skills configuration overrides.

Write only docs/post-rebuild-verification-result.md through apply_patch.
Do not change source code, manifests, generated skills, or HOME.
Do not commit, push, or start other agents.
Inspect the startup skill catalog before reading skill files.
List every catalog skill and its declared path.
Report whether any globally sourced skill appears.
Distinguish startup discovery from manual file access.

Use $codebase-design to assess SourceReference briefly.
Read its complete installed SKILL.md and all required references first.
Record one invariant and its source evidence.
Explain which skill terminology influenced the assessment.

Run hostname and readlink /run/current-system.
Resolve agent-skills with command -v and readlink -f.
Run agent-skills --version.
Run agent-skills validate --json.
Run agent-skills check --offline --json.
Use the installed CLI, not Cargo or a target-directory binary.
Record command statuses and reports.
Record whether the installed CLI resolves to the Nix store package.
Do not claim fresh downloads from an offline check.
Never read secrets or print credentials.
Use sentences with at most 20 words.
