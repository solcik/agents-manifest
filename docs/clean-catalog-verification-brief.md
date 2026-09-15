# Clean catalog verification

Route: Codex, openai-codex, gpt-5.6-sol, low reasoning.
The session explicitly disables bundled skills and the global plugin-management skill path.
These session flags test the planned NixOS configuration before host activation.

Write only docs/clean-catalog-verification-result.md through apply_patch.
Do not change source files, skills, manifests, or HOME.
Inspect the startup skill catalog before reading files.
Report every catalog skill and its declared path.
Report whether codebase-design and writing-guidelines appear at project paths.
Report whether any globally sourced skill remains.
Distinguish catalog discovery from disk existence.
Do not claim that session flags update persistent host configuration.

Use $codebase-design for a brief assessment of SourceReference.
Read its complete generated SKILL.md and required references first.
Record one validation invariant from source evidence.
Do not refactor code or spawn agents.
Never read secret files or print credentials.
Use sentences with at most 20 words.
